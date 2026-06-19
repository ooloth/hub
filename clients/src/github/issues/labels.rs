use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct ApiLabel {
    pub(super) name: String,
}

/// Replaces all labels on an issue with the given set.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable or returns a non-2xx response.
pub async fn set_issue_labels(
    token: &str,
    repo: &str,
    number: u64,
    labels: &[String],
) -> Result<()> {
    let _ = reqwest::Client::new()
        .put(format!(
            "https://api.github.com/repos/{repo}/issues/{number}/labels"
        ))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .json(&serde_json::json!({ "labels": labels }))
        .send()
        .await
        .with_context(|| format!("failed to reach GitHub API for {repo}#{number}"))?
        .error_for_status()
        .with_context(|| format!("GitHub API error setting labels on {repo}#{number}"))?;
    Ok(())
}

pub(super) async fn fetch_issue_labels(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    repo: &str,
    number: u64,
) -> Result<Vec<String>> {
    let labels: Vec<ApiLabel> = client
        .get(format!("{base}/repos/{repo}/issues/{number}/labels"))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .with_context(|| format!("failed to reach GitHub API fetching labels on {repo}#{number}"))?
        .error_for_status()
        .with_context(|| format!("GitHub API error fetching labels on {repo}#{number}"))?
        .json()
        .await
        .with_context(|| format!("failed to parse labels response for {repo}#{number}"))?;
    Ok(labels.into_iter().map(|l| l.name).collect())
}
