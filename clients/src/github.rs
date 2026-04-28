use anyhow::{Context, Result};
use chrono::Utc;
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
    created_at: String,
    labels: Vec<Label>,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

fn age_days(created_at: &str) -> u64 {
    let Ok(created) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return 0;
    };
    let days = (Utc::now() - created.to_utc()).num_days();
    days.max(0) as u64
}

pub async fn prs_awaiting_review(token: &str, repos: &[String]) -> Result<Vec<PullRequest>> {
    if repos.is_empty() {
        return Ok(vec![]);
    }
    let query = scoped_query("is:open is:pr review-requested:@me", repos);
    let response: SearchResponse = search(token, &query).await?;
    response
        .items
        .into_iter()
        .map(|item| {
            Ok(PullRequest {
                number: item.number,
                title: item.title,
                repo: repo_slug_from_url(&item.repository_url)?,
                url: item.html_url,
                age_days: age_days(&item.created_at),
            })
        })
        .collect()
}

pub async fn issues(token: &str, repos: &[String], assigned_only: bool) -> Result<Vec<Issue>> {
    if repos.is_empty() {
        return Ok(vec![]);
    }
    let base = if assigned_only {
        "is:open is:issue assignee:@me"
    } else {
        "is:open is:issue"
    };
    let query = scoped_query(base, repos);
    let response: SearchResponse = search(token, &query).await?;
    response
        .items
        .into_iter()
        .map(|item| {
            Ok(Issue {
                number: item.number,
                title: item.title,
                repo: repo_slug_from_url(&item.repository_url)?,
                url: item.html_url,
                age_days: age_days(&item.created_at),
                labels: item.labels.into_iter().map(|l| l.name).collect(),
            })
        })
        .collect()
}

fn scoped_query(base: &str, repos: &[String]) -> String {
    let repo_filters = repos
        .iter()
        .map(|r| format!("repo:{r}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{base} {repo_filters}")
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
