use anyhow::Result;
use domain::{Issue, PullRequest};

pub struct StatusReport {
    pub github_prs: Vec<PullRequest>,
    pub github_issues: Vec<Issue>,
}

pub async fn run(github_token: &str) -> Result<StatusReport> {
    let (prs, issues) = tokio::join!(
        clients::github::prs_awaiting_review(github_token),
        clients::github::issues_assigned_to_me(github_token),
    );
    Ok(StatusReport {
        github_prs: prs?,
        github_issues: issues?,
    })
}
