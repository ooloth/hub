use anyhow::{Context, Result};
use domain::PullRequest;
use serde::Deserialize;

#[derive(Deserialize)]
struct SearchResponse {
    items: Vec<GithubPr>,
}

#[derive(Deserialize)]
struct GithubPr {
    number: u64,
    title: String,
    html_url: String,
    repository_url: String,
}

pub async fn prs_awaiting_review(token: &str) -> Result<Vec<PullRequest>> {
    let response: SearchResponse = reqwest::Client::new()
        .get("https://api.github.com/search/issues")
        .query(&[("q", "is:open is:pr review-requested:@me")])
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .context("failed to reach GitHub API")?
        .error_for_status()
        .context("GitHub API returned an error")?
        .json()
        .await
        .context("failed to parse GitHub response")?;

    response
        .items
        .into_iter()
        .map(|pr| {
            Ok(PullRequest {
                number: pr.number,
                title: pr.title,
                repo: repo_name_from_url(&pr.repository_url)?,
                url: pr.html_url,
            })
        })
        .collect()
}

fn repo_name_from_url(url: &str) -> Result<String> {
    let mut parts = url.rsplit('/');
    let repo = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing repo segment in repository_url: {url}"))?;
    let owner = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing owner segment in repository_url: {url}"))?;
    Ok(format!("{owner}/{repo}"))
}
