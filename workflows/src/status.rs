use anyhow::Result;
use domain::{Issue, PullRequest};

pub struct StatusReport {
    pub github_prs: Vec<PullRequest>,
    pub github_issues: Vec<Issue>,
}

pub async fn run(
    github_token: &str,
    pr_repos: &[String],
    issue_repos: &[String],
    assigned_issue_repos: &[String],
) -> Result<StatusReport> {
    let (prs, issues, assigned_issues) = tokio::join!(
        clients::github::prs_awaiting_review(github_token, pr_repos),
        clients::github::issues(github_token, issue_repos, false),
        clients::github::issues(github_token, assigned_issue_repos, true),
    );
    let mut github_issues = issues?;
    github_issues.extend(assigned_issues?);
    Ok(StatusReport {
        github_prs: prs?,
        github_issues,
    })
}
