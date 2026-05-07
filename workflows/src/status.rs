use anyhow::Result;
use domain::{CiFailure, Issue, LinearIssue, PullRequest, Urgency};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;

pub const SCHEMA_VERSION: i32 = 4;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StatusItem {
    Pr(PullRequest),
    Issue(Issue),
    Ci(CiFailure),
    Linear(LinearIssue),
    Loki(domain::LokiErrors),
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
}

/// Fetches all status data concurrently, merges into a unified list, and sorts
/// by (urgency ascending, age descending) so the most pressing item is first.
///
/// # Errors
/// Returns an error if any API call fails.
pub async fn run(
    github_token: &str,
    pr_repos: &[String],
    issue_repos: &[String],
    assigned_issue_repos: &[String],
    ci_repos: &[(String, String)],
    linear_token: Option<&str>,
    private_workflow_names: Vec<String>,
    loki_envs: &[domain::LokiEnv],
) -> Result<StatusReport> {
    let (prs, issues, assigned_issues, ci_failures, linear_issues) = tokio::join!(
        clients::github::prs_awaiting_review(github_token, pr_repos),
        clients::github::issues(github_token, issue_repos, false),
        clients::github::issues(github_token, assigned_issue_repos, true),
        clients::github::ci_failures(github_token, ci_repos),
        async {
            match linear_token {
                Some(token) => clients::linear::issues(token).await,
                None => Ok(vec![]),
            }
        },
    );

    let mut github_issues = issues?;
    github_issues.extend(assigned_issues?);

    let mut items: Vec<StatusItem> = Vec::new();

    items.extend(prs?.into_iter().map(StatusItem::Pr));
    items.extend(github_issues.into_iter().map(StatusItem::Issue));
    items.extend(ci_failures?.into_iter().map(StatusItem::Ci));
    items.extend(linear_issues?.into_iter().map(StatusItem::Linear));

    for env in loki_envs {
        match crate::loki::run(env).await {
            Ok(errors) => items.extend(errors.into_iter().map(StatusItem::Loki)),
            Err(e) => eprintln!("loki ({} · {}): {e}", env.project, env.env),
        }
    }

    #[cfg(feature = "private")]
    {
        let private = crate::private::status::run(private_workflow_names).await?;
        if let Some(media) = private.media {
            items.extend(media.blocked.into_iter().map(StatusItem::MediaBlocked));
            items.extend(
                media
                    .recent_missing
                    .into_iter()
                    .map(StatusItem::MediaMissing),
            );
            items.extend(media.health_items.into_iter().map(StatusItem::MediaHealth));
            if media.backlog_count > 0 {
                items.push(StatusItem::MediaBacklog {
                    source: media.source.clone(),
                    count: media.backlog_count,
                });
            }
        }
    }

    #[cfg(not(feature = "private"))]
    let _ = private_workflow_names;

    items.sort_by_key(|i| (i.urgency(), Reverse(i.age())));

    Ok(StatusReport { items })
}
