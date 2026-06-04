use anyhow::Result;
use domain::{CiFailure, Issue, LinearIssue, PullRequest, Task, Urgency};
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashMap;

pub const SCHEMA_VERSION: i32 = 15;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StatusItem {
    Pr(PullRequest),
    Issue(Issue),
    Ci(CiFailure),
    Linear(LinearIssue),
    Loki(domain::LokiEntry),
    Gcp(domain::GcpEntry),
    AgentSession(Task),
    #[cfg(feature = "private")]
    MediaBlocked(crate::private::status::BlockedItem),
    #[cfg(feature = "private")]
    MediaMissing(crate::private::status::MissingItem),
    #[cfg(feature = "private")]
    MediaHealth(crate::private::status::HealthItem),
    #[cfg(feature = "private")]
    MediaBacklog {
        source: String,
        count: u32,
    },
}

impl StatusItem {
    fn urgency(&self) -> Urgency {
        match self {
            Self::Pr(pr) => pr.urgency,
            Self::Issue(i) => i.urgency,
            Self::Ci(c) => c.urgency,
            Self::Linear(l) => l.urgency,
            Self::Loki(l) => l.urgency,
            Self::Gcp(g) => g.urgency,
            Self::AgentSession(t) => t.urgency,
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

    fn age(&self) -> chrono::Duration {
        match self {
            Self::Pr(pr) => pr.age,
            Self::Issue(i) => i.age,
            Self::Ci(c) => c.age,
            Self::Linear(l) => l.age,
            Self::Loki(l) => l.age,
            Self::Gcp(g) => g.age,
            Self::AgentSession(t) => t.age,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusReport {
    pub items: Vec<StatusItem>,
    /// Names of API sources that failed during the refresh (e.g., "github ci").
    #[serde(default)]
    pub errors: Vec<String>,
}

/// Returned by the private workflow runner so source names come from data, not hub source code.
#[cfg(feature = "private")]
pub struct PrivateStatusResult {
    pub items: Vec<StatusItem>,
    pub failed_sources: Vec<String>,
}

pub struct StatusParams {
    pub github_token: Secret<String>,
    pub github_username: String,
    pub pr_repos: Vec<domain::GithubPrsRepo>,
    pub issue_repos: Vec<String>,
    pub ci_repos: Vec<(String, String)>,
    pub linear_token: Option<Secret<String>>,
    pub private_workflow_names: Vec<String>,
    pub loki_envs: Vec<domain::LokiEnv>,
    pub gcp_envs: Vec<domain::GcpEnv>,
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
            match params.linear_token.as_ref().map(|t| t.expose_secret()) {
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

    // GitHub PRs — collect errors for each category that fails.
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

    // Agent tasks — synchronous local SQLite read; degrades gracefully on error.
    match crate::tasks::list_visible() {
        Ok(tasks) => items.extend(tasks.into_iter().map(StatusItem::AgentSession)),
        Err(_) => errors.push("agent tasks".to_string()),
    }

    items.sort_by_key(|i| (i.urgency(), Reverse(i.age())));

    Ok(StatusReport { items, errors })
}
