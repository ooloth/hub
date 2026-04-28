use anyhow::{Context, Result};
use domain::{Issue, PullRequest, RepoSlug};
use serde::Deserialize;

#[derive(Deserialize)]
struct SearchResponse {
    items: Vec<SearchItem>,
}

#[derive(Deserialize)]
struct SearchItem {
    number: u64,
    title: String,
    html_url: String,
    repository_url: String,
}

pub async fn prs_awaiting_review(token: &str) -> Result<Vec<PullRequest>> {
    let response: SearchResponse = search(token, "is:open is:pr review-requested:@me").await?;
    response
        .items
        .into_iter()
        .map(|item| {
            Ok(PullRequest {
                number: item.number,
                title: item.title,
                repo: repo_slug_from_url(&item.repository_url)?,
                url: item.html_url,
            })
        })
        .collect()
}

pub async fn issues_assigned_to_me(token: &str) -> Result<Vec<Issue>> {
    let response: SearchResponse = search(token, "is:open is:issue assignee:@me").await?;
    response
        .items
        .into_iter()
        .map(|item| {
            Ok(Issue {
                number: item.number,
                title: item.title,
                repo: repo_slug_from_url(&item.repository_url)?,
                url: item.html_url,
            })
        })
        .collect()
}

async fn search(token: &str, query: &str) -> Result<SearchResponse> {
    reqwest::Client::new()
        .get("https://api.github.com/search/issues")
        .query(&[("q", query)])
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
        .context("failed to parse GitHub response")
}

// GitHub repository_url: https://api.github.com/repos/{owner}/{repo}
fn repo_slug_from_url(url: &str) -> Result<RepoSlug> {
    let after = url
        .split_once("/repos/")
        .map(|(_, rest)| rest)
        .ok_or_else(|| anyhow::anyhow!("expected '/repos/' in repository_url: {url}"))?;
    let (owner, repo) = after
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("expected 'owner/repo' after '/repos/' in: {url}"))?;
    if owner.is_empty() {
        anyhow::bail!("empty owner in repository_url: {url}");
    }
    if repo.is_empty() {
        anyhow::bail!("empty repo in repository_url: {url}");
    }
    Ok(RepoSlug::new(owner, repo))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_api_repository_url() {
        let slug = repo_slug_from_url("https://api.github.com/repos/ooloth/hub").unwrap();
        assert_eq!(slug.to_string(), "ooloth/hub");
    }

    #[test]
    fn rejects_url_without_repos_segment() {
        assert!(repo_slug_from_url("https://api.github.com/ooloth/hub").is_err());
    }

    #[test]
    fn rejects_url_missing_repo_after_owner() {
        assert!(repo_slug_from_url("https://api.github.com/repos/ooloth").is_err());
    }
}
