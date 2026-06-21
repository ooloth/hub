use anyhow::Result;
use domain::{CiFailure, Issue, LinearIssue, PullRequest, Urgency};
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashMap;

/// Bump when the serialized `StatusReport` format changes incompatibly.
pub const SCHEMA_VERSION: i32 = 17;

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

/// Fetches all status data concurrently, merges into a unified list, and sorts
/// by (urgency ascending, age descending) so the most pressing item is first.
///
/// # Errors
/// Returns an error if any API call fails.
pub async fn run(params: StatusParams) -> Result<StatusReport> {
    let github_token = params.github_token.expose_secret();
    let (my_open, review_queue, my_drafts, external, issues, ci_failures, linear_issues) = tokio::join!(
        clients::github::my_open_prs(github_token, &params.pr_repos, &params.github_username),
        clients::github::prs_awaiting_review(github_token, &params.pr_repos),
        clients::github::my_draft_prs(github_token, &params.pr_repos, &params.github_username),
        clients::github::external_prs(github_token, &params.pr_repos),
        clients::github::issues(github_token, &params.issue_repos, &params.github_username),
        clients::github::ci_failures(github_token, &params.ci_repos),
        async {
            match params
                .linear_token
                .as_ref()
                .map(secrecy::ExposeSecret::expose_secret)
            {
                Some(token) => clients::linear::issues(token).await,
                None => Ok(vec![]),
            }
        },
    );

    let mut items: Vec<StatusItem> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // GitHub Issues.
    match issues {
        Ok(all_issues) => items.extend(all_issues.into_iter().map(StatusItem::Issue)),
        Err(_) => errors.push("github issues".to_string()),
    }

    collect_prs(
        my_open,
        review_queue,
        my_drafts,
        external,
        &mut items,
        &mut errors,
    );

    // GitHub CI.
    if let Ok(ci) = ci_failures {
        items.extend(ci.into_iter().map(StatusItem::Ci));
    } else {
        errors.push("github ci failures".to_string());
    }

    // Linear issues.
    if let Ok(linear) = linear_issues {
        items.extend(linear.into_iter().map(StatusItem::Linear));
    } else {
        errors.push("linear issues".to_string());
    }

    for env in &params.loki_envs {
        match crate::loki::run(env).await {
            Ok(loki_items) => items.extend(loki_items.into_iter().map(StatusItem::Loki)),
            Err(_) => errors.push(format!("loki ({} · {})", env.project, env.env)),
        }
    }

    for env in &params.gcp_envs {
        match crate::gcp::run(env).await {
            Ok(gcp_items) => items.extend(gcp_items.into_iter().map(StatusItem::Gcp)),
            Err(_) => errors.push(format!("gcp ({} · {})", env.project, env.env)),
        }
    }

    // Private workflows — gracefully handle failures; source names come from the result data.
    #[cfg(feature = "private")]
    {
        let private =
            crate::private::status::run(params.private_workflow_names, &params.extra_credentials)
                .await;
        items.extend(private.items);
        errors.extend(private.failed_sources);
    }

    #[cfg(not(feature = "private"))]
    let _ = params.private_workflow_names;

    items.sort_by_key(|i| (i.urgency(), Reverse(i.age())));

    Ok(StatusReport { items, errors })
}

fn collect_prs(
    my_open: Result<Vec<PullRequest>>,
    review_queue: Result<Vec<PullRequest>>,
    my_drafts: Result<Vec<PullRequest>>,
    external: Result<Vec<PullRequest>>,
    items: &mut Vec<StatusItem>,
    errors: &mut Vec<String>,
) {
    let mut all_prs: Vec<PullRequest> = Vec::new();
    if let Ok(prs) = my_open {
        all_prs.extend(prs);
    } else {
        errors.push("github my open prs".to_string());
    }
    if let Ok(prs) = review_queue {
        all_prs.extend(prs);
    } else {
        errors.push("github prs awaiting review".to_string());
    }
    if let Ok(prs) = my_drafts {
        all_prs.extend(prs);
    } else {
        errors.push("github my draft prs".to_string());
    }
    if let Ok(prs) = external {
        all_prs.extend(prs);
    } else {
        errors.push("github external prs".to_string());
    }
    // A PR can match multiple queries (e.g. author + review-requested); keep first occurrence.
    let mut seen_prs: std::collections::HashSet<(String, u64)> = std::collections::HashSet::new();
    all_prs.retain(|pr| seen_prs.insert((pr.repo.to_string(), pr.number)));
    items.extend(all_prs.into_iter().map(StatusItem::Pr));
}
