use anyhow::Result;
use domain::PullRequest;

pub struct StatusReport {
    pub github_prs: Vec<PullRequest>,
}

pub async fn run(github_token: &str) -> Result<StatusReport> {
    let prs = clients::github::prs_awaiting_review(github_token).await?;
    Ok(StatusReport { github_prs: prs })
}
