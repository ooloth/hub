use anyhow::Result;
use domain::{CiFailure, Issue, LinearIssue, PullRequest, Urgency};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;

pub const SCHEMA_VERSION: i32 = 7;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StatusItem {
    Pr(PullRequest),
    Issue(Issue),
    Ci(CiFailure),
    Linear(LinearIssue),
    Loki(domain::LokiEntry),
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
    /// API sources that failed during the refresh (e.g., "sonarr", "github")
    #[serde(default)]
    pub errors: Vec<String>,
}

pub struct StatusParams {
    pub github_token: String,
    pub github_username: String,
    pub pr_repos: Vec<String>,
    pub issue_repos: Vec<String>,
    pub assigned_issue_repos: Vec<String>,
    pub ci_repos: Vec<(String, String)>,
    pub linear_token: Option<String>,
    pub private_workflow_names: Vec<String>,
    pub loki_envs: Vec<domain::LokiEnv>,
}

/// Fetches all status data concurrently, merges into a unified list, and sorts
/// by (urgency ascending, age descending) so the most pressing item is first.
///
/// # Errors
/// Returns an error if any API call fails.
pub async fn run(params: StatusParams) -> Result<StatusReport> {
    let (my_open, review_queue, my_drafts, issues, assigned_issues, ci_failures, linear_issues) = tokio::join!(
        clients::github::my_open_prs(
            &params.github_token,
            &params.pr_repos,
            &params.github_username,
        ),
        clients::github::prs_awaiting_review(&params.github_token, &params.pr_repos),
        clients::github::my_draft_prs(
            &params.github_token,
            &params.pr_repos,
            &params.github_username,
        ),
        clients::github::issues(&params.github_token, &params.issue_repos, false),
        clients::github::issues(&params.github_token, &params.assigned_issue_repos, true),
        clients::github::ci_failures(&params.github_token, &params.ci_repos),
        async {
            match params.linear_token.as_deref() {
                Some(token) => clients::linear::issues(token).await,
                None => Ok(vec![]),
            }
        },
    );

    let mut items: Vec<StatusItem> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // GitHub Issues — combine open and assigned issues.
    match issues {
        Ok(mut all_issues) => {
            if let Ok(assigned) = assigned_issues {
                all_issues.extend(assigned);
            } else {
                errors.push("github assigned issues".to_string());
            }
            items.extend(all_issues.into_iter().map(StatusItem::Issue));
        }
        Err(_) => {
            // Both open and assigned failed; only report once.
            errors.push("github issues".to_string());
            // If assigned succeeded even though open failed, still add them.
            if let Ok(assigned) = assigned_issues {
                items.extend(assigned.into_iter().map(StatusItem::Issue));
            }
        }
    }

    // GitHub PRs — collect errors for each category that fails.
    if let Ok(prs) = my_open {
        items.extend(prs.into_iter().map(StatusItem::Pr));
    } else {
        errors.push("github my open prs".to_string());
    }

    if let Ok(prs) = review_queue {
        items.extend(prs.into_iter().map(StatusItem::Pr));
    } else {
        errors.push("github prs awaiting review".to_string());
    }

    if let Ok(prs) = my_drafts {
        items.extend(prs.into_iter().map(StatusItem::Pr));
    } else {
        errors.push("github my draft prs".to_string());
    }

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

    // Loki errors are logged but don't fail the refresh (like the original behavior).
    for env in &params.loki_envs {
        match crate::loki::run(env).await {
            Ok(loki_items) => items.extend(loki_items.into_iter().map(StatusItem::Loki)),
            Err(e) => eprintln!("loki ({} · {}): {e}", env.project, env.env),
        }
    }

    // Private workflows (sonarr, etc.) — gracefully handle failures.
    #[cfg(feature = "private")]
    {
        match crate::private::status::run(params.private_workflow_names).await {
            Ok(private) => {
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
            Err(_) => {
                errors.push("sonarr".to_string());
            }
        }
    }

    #[cfg(not(feature = "private"))]
    let _ = params.private_workflow_names;

    items.sort_by_key(|i| (i.urgency(), Reverse(i.age())));

    Ok(StatusReport { items, errors })
}
