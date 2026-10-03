use anyhow::Result;
use domain::{CiFailure, Issue, LinearIssue, PullRequest, Urgency};
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::time::Duration;

use crate::sources::{gather, Source, SourceAnswer};

/// The longest a refresh waits for any one source.
///
/// The slowest whole pass measured on 2026-10-02 took 9.2 seconds, so this leaves a wide
/// margin while a source that never answers still cannot hold back the rest. See #338.
pub const SOURCE_TIMEOUT: Duration = Duration::from_mins(1);

/// Bump when the serialized `StatusReport` format changes incompatibly.
pub const SCHEMA_VERSION: i32 = 18;

/// A single signal from any source, as stored in the unified status list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StatusItem {
    /// A GitHub pull request.
    Pr(PullRequest),
    /// A GitHub or Linear issue.
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
    /// Names of API sources that failed during the refresh (e.g., "github ci").
    #[serde(default)]
    pub errors: Vec<String>,
}

/// Returned by the private workflow runner so source names come from data, not hub source code.
#[cfg(feature = "private")]
#[derive(Debug)]
pub struct PrivateStatusResult {
    /// Items collected from private sources.
    pub items: Vec<StatusItem>,
    /// Names of private sources that failed.
    pub failed_sources: Vec<String>,
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
/// Never in practice: a source that fails or runs out of time is not an error. Its name is
/// collected into [`StatusReport::errors`] and the remaining sources still contribute.
pub async fn run(params: StatusParams) -> Result<StatusReport> {
    let gathered = gather(sources(params), SOURCE_TIMEOUT).await;
    let mut items = gathered.items;
    dedupe_prs(&mut items);
    items.sort_by_key(|i| (i.urgency(), Reverse(i.age())));
    Ok(StatusReport {
        items,
        errors: gathered.failed,
    })
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
    sources.push(Source::new("github my open prs", async move {
        let prs = clients::github::my_open_prs(token.expose_secret(), &repos, &user).await?;
        Ok(answer(prs.into_iter().map(StatusItem::Pr)))
    }));

    let (token, repos) = (github_token.clone(), pr_repos.clone());
    sources.push(Source::new("github prs awaiting review", async move {
        let prs = clients::github::prs_awaiting_review(token.expose_secret(), &repos).await?;
        Ok(answer(prs.into_iter().map(StatusItem::Pr)))
    }));

    let (token, repos, user) = (github_token.clone(), pr_repos.clone(), github_username);
    sources.push(Source::new("github my draft prs", async move {
        let prs = clients::github::my_draft_prs(token.expose_secret(), &repos, &user).await?;
        Ok(answer(prs.into_iter().map(StatusItem::Pr)))
    }));

    let (token, repos) = (github_token.clone(), pr_repos);
    sources.push(Source::new("github external prs", async move {
        let prs = clients::github::external_prs(token.expose_secret(), &repos).await?;
        Ok(answer(prs.into_iter().map(StatusItem::Pr)))
    }));

    let (token, repos) = (github_token, ci_repos);
    sources.push(Source::new("github ci failures", async move {
        let failures = clients::github::ci_failures(token.expose_secret(), &repos).await?;
        Ok(answer(failures.into_iter().map(StatusItem::Ci)))
    }));

    sources.push(Source::new("linear issues", async move {
        let issues = match linear_token {
            Some(token) => clients::linear::issues(token.expose_secret()).await?,
            None => vec![],
        };
        Ok(answer(issues.into_iter().map(StatusItem::Linear)))
    }));

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
        Ok(SourceAnswer {
            items: result.items,
            failed: result.failed_sources,
        })
    }));
    #[cfg(not(feature = "private"))]
    let _ = (private_workflow_names, extra_credentials);

    sources
}

/// An answer holding `items` and no failures of its own.
fn answer(items: impl IntoIterator<Item = StatusItem>) -> SourceAnswer {
    SourceAnswer {
        items: items.into_iter().collect(),
        failed: vec![],
    }
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

    fn params() -> StatusParams {
        StatusParams {
            github_token: Secret::new("token".to_string()),
            github_username: "me".to_string(),
            pr_repos: vec![],
            issue_repos: vec![],
            ci_repos: vec![],
            linear_token: None,
            private_workflow_names: vec![],
            loki_envs: vec![domain::LokiEnv {
                project: "app".to_string(),
                env: "prod".to_string(),
                endpoint: "https://loki.invalid".to_string(),
                token: None,
                grafana_url: None,
                queries: vec![],
            }],
            gcp_envs: vec![domain::GcpEnv {
                project: "api".to_string(),
                env: "staging".to_string(),
                gcp_project: "gcp-project".to_string(),
                gcp_region: None,
                queries: vec![],
            }],
            extra_credentials: HashMap::new(),
        }
    }

    #[test]
    fn sources_are_named_as_the_report_has_always_named_them() {
        let names: Vec<String> = sources(params())
            .into_iter()
            .map(|source| source.name)
            .collect();

        #[allow(unused_mut)]
        let mut expected = vec![
            "github issues",
            "github my open prs",
            "github prs awaiting review",
            "github my draft prs",
            "github external prs",
            "github ci failures",
            "linear issues",
            "loki (app · prod)",
            "gcp (api · staging)",
        ];
        #[cfg(feature = "private")]
        expected.push("private workflows");
        assert_eq!(names, expected);
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
            author: "alice".to_string(),
            review_decision: None,
            approval_count: 0,
            changes_requested_count: 0,
            comment_count: 0,
            head_branch: "feat".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: None,
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
}
