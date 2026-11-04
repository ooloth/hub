use anyhow::Result;
use domain::{CiFailure, Issue, LinearIssue, PullRequest, Urgency};
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::time::Duration;

use crate::sources::{gather, Gathered, Source, SourceAnswer};
use domain::known_secrets::KnownSecrets;
use domain::source_failure::SourceFailure;
use domain::source_outcomes::SourceOutcomes;

/// The longest a refresh waits for any one source.
///
/// The slowest whole pass measured on 2026-10-02 took 9.2 seconds, so this leaves a wide
/// margin while a source that never answers still cannot hold back the rest. See #338.
pub const SOURCE_TIMEOUT: Duration = Duration::from_mins(1);

/// The source that reports my open pull requests, and so which of mine are my turn.
pub const MY_OPEN_PRS: &str = "github my open prs";

/// The source that reports my draft pull requests, each of which is my turn.
pub const MY_DRAFT_PRS: &str = "github my draft prs";

/// The source that reports pull requests whose review is requested from me.
pub const PRS_AWAITING_REVIEW: &str = "github prs awaiting review";

/// Bump when the serialized `StatusReport` format changes incompatibly.
///
/// The test `the_cached_payload_format_is_pinned_to_its_schema_version` fails when the format
/// changes, and `ui/tui/README.md` says which changes need a bump.
pub const SCHEMA_VERSION: i32 = 22;

/// A single signal from any source, as stored in the unified status list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StatusItem {
    /// A GitHub pull request.
    Pr(PullRequest),
    /// A GitHub issue.
    Issue(Issue),
    /// A failed CI run.
    Ci(CiFailure),
    /// A Linear issue.
    Linear(LinearIssue),
    /// A Loki log alert entry.
    Loki(domain::LokiEntry),
    /// A GCP log alert entry.
    Gcp(domain::GcpEntry),
    /// A blocked media download (private).
    #[cfg(feature = "private")]
    MediaBlocked(crate::private::status::BlockedItem),
    /// A missing media episode (private).
    #[cfg(feature = "private")]
    MediaMissing(crate::private::status::MissingItem),
    /// A media server health issue (private).
    #[cfg(feature = "private")]
    MediaHealth(crate::private::status::HealthItem),
    /// Backlog count for a media source (private).
    #[cfg(feature = "private")]
    MediaBacklog {
        /// The name of the media source.
        source: String,
        /// Number of items in the backlog.
        count: u32,
    },
}

impl StatusItem {
    const fn urgency(&self) -> Urgency {
        match self {
            Self::Pr(pr) => pr.urgency,
            Self::Issue(i) => i.urgency,
            Self::Ci(c) => c.urgency,
            Self::Linear(l) => l.urgency,
            Self::Loki(l) => l.urgency,
            Self::Gcp(g) => g.urgency,
            #[cfg(feature = "private")]
            Self::MediaBlocked(b) => b.urgency,
            #[cfg(feature = "private")]
            Self::MediaMissing(m) => m.urgency,
            #[cfg(feature = "private")]
            Self::MediaHealth(h) => h.urgency,
            #[cfg(feature = "private")]
            Self::MediaBacklog { .. } => Urgency::Low,
        }
    }

    const fn age(&self) -> chrono::Duration {
        match self {
            Self::Pr(pr) => pr.age,
            Self::Issue(i) => i.age,
            Self::Ci(c) => c.age,
            Self::Linear(l) => l.age,
            Self::Loki(l) => l.age,
            Self::Gcp(g) => g.age,
            #[cfg(feature = "private")]
            Self::MediaBlocked(b) => b.age,
            #[cfg(feature = "private")]
            Self::MediaMissing(m) => m.age,
            #[cfg(feature = "private")]
            Self::MediaHealth(h) => h.age,
            #[cfg(feature = "private")]
            Self::MediaBacklog { .. } => chrono::Duration::zero(),
        }
    }
}

/// The complete status payload returned by a full refresh.
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusReport {
    /// All signal items across every configured source.
    pub items: Vec<StatusItem>,
    /// Names of API sources that failed during the refresh (e.g., "github ci failures").
    #[serde(default)]
    pub errors: Vec<String>,
}

/// One refresh: the report the cache stores, and why each failed source failed.
///
/// The reasons travel beside the report rather than inside it, so the cached payload keeps its
/// shape. `report.errors` names the same sources as `sources.failed()`, in the same order.
#[derive(Debug)]
pub struct Refresh {
    /// What the cache stores.
    pub report: StatusReport,
    /// Which sources answered, and each that failed with its reason redacted of every credential.
    pub sources: SourceOutcomes,
}

/// Returned by the private workflow runner so source names come from data, not hub source code.
#[cfg(feature = "private")]
#[derive(Debug)]
pub(crate) struct PrivateStatusResult {
    /// Items collected from private sources.
    pub(crate) items: Vec<StatusItem>,
    /// Private sources that answered.
    pub(crate) answered: Vec<String>,
    /// Private sources that failed, with the error each failed with.
    pub(crate) failures: Vec<crate::sources::SourceError>,
}

/// All credentials and configuration needed for a full status refresh.
#[derive(Debug)]
pub struct StatusParams {
    /// GitHub API token.
    pub github_token: Secret<String>,
    /// GitHub username for PR ownership filtering.
    pub github_username: String,
    /// GitHub repositories to fetch PRs from.
    pub pr_repos: Vec<domain::GithubPrsRepo>,
    /// GitHub repositories to fetch issues from.
    pub issue_repos: Vec<String>,
    /// `(owner/repo, workflow_name)` pairs for CI failure fetching.
    pub ci_repos: Vec<(String, String)>,
    /// Linear API token, if Linear is configured.
    pub linear_token: Option<Secret<String>>,
    /// Names of private workflows to run.
    pub private_workflow_names: Vec<String>,
    /// Loki environments to query.
    pub loki_envs: Vec<domain::LokiEnv>,
    /// GCP environments to query.
    pub gcp_envs: Vec<domain::GcpEnv>,
    /// Extra named credentials passed to private workflows.
    pub extra_credentials: HashMap<String, Secret<String>>,
}

/// Asks every configured source at once, each within [`SOURCE_TIMEOUT`], merges the answers
/// into one list and sorts it by (urgency ascending, age descending), so the most pressing
/// item is first.
///
/// # Errors
/// Never in practice: a source that fails or runs out of time is not an error. It is collected
/// into [`Refresh::sources`] with its reason, and the remaining sources still contribute.
pub async fn run(params: StatusParams) -> Result<Refresh> {
    // Before the sources take the credentials.
    let secrets = known_secrets(&params);
    let gathered = gather(sources(params), SOURCE_TIMEOUT).await;
    Ok(refresh_from(gathered, &secrets))
}

/// Every credential a refresh is given, which is everything a failure reason, a source's or the
/// whole pass's, must not contain.
#[must_use]
pub fn known_secrets(params: &StatusParams) -> KnownSecrets {
    let tokens = std::iter::once(&params.github_token)
        .chain(params.linear_token.as_ref())
        .chain(params.loki_envs.iter().filter_map(|env| env.token.as_ref()))
        .chain(params.extra_credentials.values());
    KnownSecrets::new(tokens.map(|secret| secret.expose_secret().clone()))
}

/// The refresh `gathered` amounts to: its items deduplicated and ranked, and each failure's
/// reason redacted of `secrets`.
fn refresh_from(gathered: Gathered, secrets: &KnownSecrets) -> Refresh {
    let Gathered {
        mut items,
        answered,
        errors,
    } = gathered;
    dedupe_prs(&mut items);
    items.sort_by_key(|i| (i.urgency(), Reverse(i.age())));
    let failures: Vec<SourceFailure> = errors
        .iter()
        .map(|failed| SourceFailure::new(failed.source.clone(), &failed.error, secrets))
        .collect();
    let errors = failures
        .iter()
        .map(|failure| failure.source().to_string())
        .collect();
    Refresh {
        report: StatusReport { items, errors },
        sources: SourceOutcomes::new(answered, failures),
    }
}

/// Every source a refresh asks, named as its failure would be reported, in report order.
fn sources(params: StatusParams) -> Vec<Source> {
    let StatusParams {
        github_token,
        github_username,
        pr_repos,
        issue_repos,
        ci_repos,
        linear_token,
        private_workflow_names,
        loki_envs,
        gcp_envs,
        extra_credentials,
    } = params;
    let mut sources = Vec::new();

    let (token, repos, user) = (github_token.clone(), issue_repos, github_username.clone());
    sources.push(Source::new("github issues", async move {
        let issues = clients::github::issues(token.expose_secret(), &repos, &user).await?;
        Ok(answer(issues.into_iter().map(StatusItem::Issue)))
    }));

    let (token, repos, user) = (
        github_token.clone(),
        pr_repos.clone(),
        github_username.clone(),
    );
    sources.push(Source::new(MY_OPEN_PRS, async move {
        let prs = clients::github::my_open_prs(token.expose_secret(), &repos, &user).await?;
        Ok(answer(prs.into_iter().map(StatusItem::Pr)))
    }));

    let (token, repos, user) = (
        github_token.clone(),
        pr_repos.clone(),
        github_username.clone(),
    );
    sources.push(Source::new(PRS_AWAITING_REVIEW, async move {
        let prs =
            clients::github::prs_awaiting_review(token.expose_secret(), &repos, &user).await?;
        Ok(answer(prs.into_iter().map(StatusItem::Pr)))
    }));

    let (token, repos, user) = (
        github_token.clone(),
        pr_repos.clone(),
        github_username.clone(),
    );
    sources.push(Source::new(MY_DRAFT_PRS, async move {
        let prs = clients::github::my_draft_prs(token.expose_secret(), &repos, &user).await?;
        Ok(answer(prs.into_iter().map(StatusItem::Pr)))
    }));

    let (token, repos, user) = (github_token.clone(), pr_repos, github_username);
    sources.push(Source::new("github external prs", async move {
        let prs = clients::github::external_prs(token.expose_secret(), &repos, &user).await?;
        Ok(answer(prs.into_iter().map(StatusItem::Pr)))
    }));

    let (token, repos) = (github_token, ci_repos);
    sources.push(Source::new("github ci failures", async move {
        let failures = clients::github::ci_failures(token.expose_secret(), &repos).await?;
        Ok(answer(failures.into_iter().map(StatusItem::Ci)))
    }));

    // Without a token Linear is not configured, so it is not asked. Asking would count an empty
    // answer as Linear having answered, and an outage of every other source would then read as a
    // partial refresh.
    if let Some(token) = linear_token {
        sources.push(Source::new("linear issues", async move {
            let issues = clients::linear::issues(token.expose_secret()).await?;
            Ok(answer(issues.into_iter().map(StatusItem::Linear)))
        }));
    }

    for env in loki_envs {
        let name = format!("loki ({} · {})", env.project, env.env);
        sources.push(Source::new(name, async move {
            let entries = crate::loki::run(&env).await?;
            Ok(answer(entries.into_iter().map(StatusItem::Loki)))
        }));
    }

    for env in gcp_envs {
        let name = format!("gcp ({} · {})", env.project, env.env);
        sources.push(Source::new(name, async move {
            let entries = crate::gcp::run(&env).await?;
            Ok(answer(entries.into_iter().map(StatusItem::Gcp)))
        }));
    }

    // Source names inside the private workflows come from their own results, not hub's code.
    #[cfg(feature = "private")]
    sources.push(Source::new("private workflows", async move {
        let result = crate::private::status::run(private_workflow_names, &extra_credentials).await;
        Ok(SourceAnswer::Several {
            items: result.items,
            answered: result.answered,
            failed: result.failures,
        })
    }));
    #[cfg(not(feature = "private"))]
    let _ = (private_workflow_names, extra_credentials);

    sources
}

/// A source's own answer, holding `items`.
fn answer(items: impl IntoIterator<Item = StatusItem>) -> SourceAnswer {
    SourceAnswer::Own(items.into_iter().collect())
}

/// Keeps the first occurrence of each pull request. A PR can match several queries
/// (author and review-requested, say).
fn dedupe_prs(items: &mut Vec<StatusItem>) {
    let mut seen: std::collections::HashSet<(String, u64)> = std::collections::HashSet::new();
    items.retain(|item| match item {
        StatusItem::Pr(pr) => seen.insert((pr.repo.to_string(), pr.number)),
        _ => true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loki_env(token: Option<Secret<String>>) -> domain::LokiEnv {
        domain::LokiEnv {
            project: "app".to_string(),
            repo: domain::RepoSlug::new("ooloth", "app"),
            env: "prod".to_string(),
            endpoint: "https://loki.invalid".to_string(),
            token,
            grafana_url: None,
            queries: vec![],
        }
    }

    fn params() -> StatusParams {
        StatusParams {
            github_token: Secret::new("token".to_string()),
            github_username: "me".to_string(),
            pr_repos: vec![],
            issue_repos: vec![],
            ci_repos: vec![],
            linear_token: None,
            private_workflow_names: vec![],
            loki_envs: vec![loki_env(None)],
            gcp_envs: vec![domain::GcpEnv {
                project: "api".to_string(),
                repo: domain::RepoSlug::new("ooloth", "api"),
                env: "staging".to_string(),
                gcp_project: "gcp-project".to_string(),
                gcp_region: None,
                queries: vec![],
            }],
            extra_credentials: HashMap::new(),
        }
    }

    fn source_names(params: StatusParams) -> Vec<String> {
        sources(params)
            .into_iter()
            .map(|source| source.name)
            .collect()
    }

    #[test]
    fn sources_are_named_as_the_report_has_always_named_them() {
        let expected: Vec<&str> = [
            "github issues",
            "github my open prs",
            "github prs awaiting review",
            "github my draft prs",
            "github external prs",
            "github ci failures",
            "loki (app · prod)",
            "gcp (api · staging)",
        ]
        .into_iter()
        .chain(cfg!(feature = "private").then_some("private workflows"))
        .collect();

        assert_eq!(source_names(params()), expected);
    }

    #[test]
    fn linear_is_asked_only_when_a_linear_token_is_configured() {
        let names = source_names(StatusParams {
            linear_token: Some(Secret::new("lin-token-value".to_string())),
            ..params()
        });

        assert!(
            names.iter().any(|name| name == "linear issues"),
            "{names:?}"
        );
    }

    fn failed(source: &str, error: &str) -> crate::sources::SourceError {
        crate::sources::SourceError::new(source, anyhow::anyhow!(error.to_string()))
    }

    #[test]
    fn the_report_names_the_failed_sources_in_the_order_they_failed() {
        let gathered = Gathered {
            items: vec![],
            answered: vec![],
            errors: vec![
                failed("github issues", "401"),
                failed("linear issues", "timeout"),
            ],
        };

        let refresh = refresh_from(gathered, &KnownSecrets::new(Vec::new()));

        let failed_sources: Vec<&str> = refresh
            .sources
            .failed()
            .iter()
            .map(SourceFailure::source)
            .collect();
        assert_eq!(refresh.report.errors, failed_sources);
        assert_eq!(
            refresh.report.errors,
            vec!["github issues", "linear issues"]
        );
    }

    #[test]
    fn the_refresh_names_the_sources_that_answered_beside_those_that_failed() {
        let gathered = Gathered {
            items: vec![],
            answered: vec!["github issues".to_string(), "loki (app · prod)".to_string()],
            errors: vec![failed("linear issues", "timeout")],
        };

        let refresh = refresh_from(gathered, &KnownSecrets::new(Vec::new()));

        assert_eq!(
            refresh.sources.answered(),
            ["github issues", "loki (app · prod)"]
        );
        assert_eq!(refresh.report.errors, vec!["linear issues"]);
    }

    #[test]
    fn every_credential_a_refresh_is_given_is_redacted_from_failure_reasons() {
        let params = StatusParams {
            github_token: Secret::new("ghp-token-value".to_string()),
            linear_token: Some(Secret::new("lin-token-value".to_string())),
            loki_envs: vec![loki_env(Some(Secret::new("loki-token-value".to_string())))],
            extra_credentials: HashMap::from([(
                "service_url".to_string(),
                Secret::new("https://media.internal:8989".to_string()),
            )]),
            ..params()
        };
        let gathered = Gathered {
            items: vec![],
            answered: vec![],
            errors: vec![failed(
                "every source",
                "sent ghp-token-value lin-token-value loki-token-value to media.internal",
            )],
        };

        let refresh = refresh_from(gathered, &known_secrets(&params));

        let reason = refresh
            .sources
            .failed()
            .first()
            .unwrap()
            .reason()
            .as_str()
            .to_string();
        assert_eq!(
            reason,
            "sent [redacted] [redacted] [redacted] to [redacted]"
        );
    }

    fn pr(number: u64, title: &str) -> StatusItem {
        StatusItem::Pr(PullRequest {
            number,
            title: title.to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: format!("https://github.com/owner/repo/pull/{number}"),
            age: chrono::Duration::zero(),
            urgency: Urgency::Medium,
            kind: domain::PrKind::ToReview,
            draft: false,
            author: "alice".to_string(),
            review_decision: None,
            comment_count: 0,
            head_branch: "feat".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_readiness: domain::MergeReadiness::NotYetKnown,
            activity: domain::pr_activity::PrActivity::default(),
        })
    }

    fn titles(items: &[StatusItem]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                StatusItem::Pr(pr) => pr.title.clone(),
                _ => "not a pr".to_string(),
            })
            .collect()
    }

    #[test]
    fn a_pr_found_by_two_queries_is_kept_once_at_its_first_occurrence() {
        let mut items = vec![pr(1, "first match"), pr(2, "other"), pr(1, "second match")];

        dedupe_prs(&mut items);

        assert_eq!(titles(&items), vec!["first match", "other"]);
    }

    /// A fixed moment, so the snapshot does not change from run to run.
    fn at(minute: u32) -> chrono::DateTime<chrono::Utc> {
        format!("2026-11-01T12:{minute:02}:00Z").parse().unwrap()
    }

    /// A pull request with every optional field set, every list holding an entry, and each
    /// variant of who can take part in its activity.
    fn pull_request_with_everything_set() -> StatusItem {
        use domain::pr_activity::{
            LatestVerdict, OpenThread, Opinion, Participant, PrActivity, Requested, ReviewBody,
        };

        StatusItem::Pr(PullRequest {
            number: 7,
            title: "a pull request".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/7".to_string(),
            age: chrono::Duration::hours(3),
            urgency: Urgency::High,
            kind: domain::PrKind::Mine,
            draft: true,
            author: "alice".to_string(),
            review_decision: Some(domain::ReviewDecision::ChangesRequested),
            comment_count: 2,
            head_branch: "feat".to_string(),
            base_branch: "main".to_string(),
            body: Some("the body".to_string()),
            ci_status: Some(domain::CiStatus::Failure),
            changed_files: vec![domain::ChangedFile {
                path: "src/lib.rs".to_string(),
                additions: 4,
                deletions: 1,
                patch: Some("@@ -1 +1 @@".to_string()),
            }],
            total_changed_files: 3,
            review_threads: vec![domain::ReviewThread {
                path: "src/lib.rs".to_string(),
                line: Some(12),
                comments: vec![domain::ReviewComment {
                    author: "bob".to_string(),
                    age: chrono::Duration::minutes(30),
                    body: "a thread comment".to_string(),
                }],
            }],
            pr_comments: vec![domain::ReviewComment {
                author: "carol".to_string(),
                age: chrono::Duration::minutes(20),
                body: "a top-level comment".to_string(),
            }],
            merge_readiness: domain::MergeReadiness::Blocked(domain::MergeBlocker::Conflict),
            activity: PrActivity {
                requests: vec![
                    Requested::Person("bob".to_string()),
                    Requested::Bot("a-bot".to_string()),
                    Requested::Team("a-team".to_string()),
                ],
                open_threads: OpenThread::from_comments(&[
                    (Participant::Person("bob".to_string()), at(1)),
                    (Participant::Bot("a-bot".to_string()), at(2)),
                    (Participant::Me, at(3)),
                ])
                .into_iter()
                .collect(),
                verdicts: vec![
                    LatestVerdict {
                        by: Participant::Person("bob".to_string()),
                        opinion: Opinion::Approved,
                        at: at(4),
                    },
                    LatestVerdict {
                        by: Participant::Bot("a-bot".to_string()),
                        opinion: Opinion::ChangesRequested,
                        at: at(5),
                    },
                ],
                bodies: vec![ReviewBody {
                    by: "carol".to_string(),
                    at: at(6),
                }],
                my_latest: Some(at(7)),
            },
        })
    }

    /// One signal of each public kind, with every optional field set and every list holding
    /// an entry.
    fn every_public_signal() -> Vec<StatusItem> {
        let repo = domain::RepoSlug::new("owner", "repo");
        vec![
            pull_request_with_everything_set(),
            StatusItem::Issue(Issue {
                number: 8,
                title: "an issue".to_string(),
                repo: repo.clone(),
                url: "https://github.com/owner/repo/issues/8".to_string(),
                author: "dave".to_string(),
                age: chrono::Duration::days(2),
                urgency: Urgency::Low,
                labels: vec!["bug".to_string()],
                body: Some("the issue body".to_string()),
            }),
            StatusItem::Ci(CiFailure {
                repo: repo.clone(),
                workflow_name: "CI".to_string(),
                job_name: Some("check".to_string()),
                step_name: Some("clippy".to_string()),
                error: Some("a warning".to_string()),
                age: chrono::Duration::minutes(5),
                urgency: Urgency::High,
                url: "https://github.com/owner/repo/actions/runs/1".to_string(),
            }),
            StatusItem::Linear(LinearIssue {
                identifier: "ENG-1".to_string(),
                title: "a linear issue".to_string(),
                url: "https://linear.app/team/issue/ENG-1".to_string(),
                state: "In Progress".to_string(),
                age: chrono::Duration::days(1),
                urgency: Urgency::Medium,
            }),
            StatusItem::Loki(domain::LokiEntry {
                title: "app errors".to_string(),
                project: "myapp".to_string(),
                repo: repo.clone(),
                env: "prod".to_string(),
                message: domain::UntrustedText::new("a loki message"),
                line: domain::UntrustedText::new("{\"message\":\"a loki message\"}"),
                lookback: "15m".to_string(),
                age: chrono::Duration::minutes(2),
                urgency: Urgency::High,
                url: "https://grafana.example.com/explore".to_string(),
            }),
            StatusItem::Gcp(domain::GcpEntry {
                title: "worker errors".to_string(),
                project: "myapp".to_string(),
                repo,
                env: "staging".to_string(),
                message: domain::UntrustedText::new("a gcp message"),
                line: domain::UntrustedText::new("{\"message\":\"a gcp message\"}"),
                lookback: "1h".to_string(),
                age: chrono::Duration::minutes(9),
                urgency: Urgency::Medium,
                url: "https://console.cloud.google.com/logs".to_string(),
                gcp_project: "myapp-staging".to_string(),
            }),
        ]
    }

    /// The cached payload's format, under the schema version it is stored with. It holds one
    /// signal of each public kind with every field filled, so renaming, adding or removing a
    /// field shows here, and so does renaming any enum variant the payload uses. Renaming a
    /// variant it does not use, and the private signals, which public CI cannot build, do not
    /// show. Accepting a change here is where to decide whether `SCHEMA_VERSION` needs a bump,
    /// by the rules in `ui/tui/README.md`.
    #[test]
    fn the_cached_payload_format_is_pinned_to_its_schema_version() {
        let report = StatusReport {
            items: every_public_signal(),
            errors: vec!["github issues".to_string()],
        };

        insta::assert_json_snapshot!(serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "payload": report,
        }));
    }
}
