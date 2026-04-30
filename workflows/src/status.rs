use anyhow::Result;
use domain::{Issue, LinearIssue, PullRequest};

pub struct StatusReport {
    pub github_prs: Vec<PullRequest>,
    pub github_issues: Vec<Issue>,
    pub linear_issues: Vec<LinearIssue>,
}

/// Fetches all status data concurrently and returns a combined report.
///
/// # Errors
/// Returns an error if any API call fails.
pub async fn run(
    github_token: &str,
    pr_repos: &[String],
    issue_repos: &[String],
    assigned_issue_repos: &[String],
    linear_token: Option<&str>,
) -> Result<StatusReport> {
    let (prs, issues, assigned_issues, linear_issues) = tokio::join!(
        clients::github::prs_awaiting_review(github_token, pr_repos),
        clients::github::issues(github_token, issue_repos, false),
        clients::github::issues(github_token, assigned_issue_repos, true),
        async {
            match linear_token {
                Some(token) => clients::linear::issues(token).await,
                None => Ok(vec![]),
            }
        },
    );

    let mut github_issues = issues?;
    github_issues.extend(assigned_issues?);

    Ok(StatusReport {
        github_prs: prs?,
        github_issues,
        linear_issues: linear_issues?,
    })
}
