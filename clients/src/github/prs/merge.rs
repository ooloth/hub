use anyhow::{Context, Result};

/// Squash-merges a pull request.
///
/// Returns an error if the PR is not mergeable, has a merge conflict,
/// or the GitHub API is unreachable.
pub async fn merge_pull_request(token: &str, repo: &str, number: u64) -> Result<()> {
    reqwest::Client::new()
        .put(format!(
            "https://api.github.com/repos/{repo}/pulls/{number}/merge"
        ))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .json(&serde_json::json!({ "merge_method": "squash" }))
        .send()
        .await
        .with_context(|| format!("failed to reach GitHub API merging {repo}#{number}"))?
        .error_for_status()
        .with_context(|| format!("GitHub API error merging {repo}#{number}"))?;
    Ok(())
}
