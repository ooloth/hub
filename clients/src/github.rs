use anyhow::{Context, Result};
use chrono::Utc;
use domain::{CiFailure, Issue, PullRequest, RepoSlug};
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
    days.max(0).cast_unsigned()
}

/// Returns PRs across the given repos where review has been requested from the authenticated user.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable or returns a non-2xx response.
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

/// Returns open issues across the given repos, optionally filtered to only assigned issues.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable or returns a non-2xx response.
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

// ── CI status ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RepoInfo {
    default_branch: String,
}

#[derive(Deserialize)]
struct RunsResponse {
    workflow_runs: Vec<WorkflowRun>,
}

#[derive(Deserialize, Clone)]
struct WorkflowRun {
    path: String,
    name: String,
    head_branch: String,
    conclusion: Option<String>,
    created_at: String,
    html_url: String,
}

const FAILING_CONCLUSIONS: &[&str] =
    &["failure", "timed_out", "startup_failure", "action_required"];

/// Returns the latest failed CI run per workflow file for each configured repo.
///
/// Only considers runs on the default branch completed within the lookback window.
///
/// # Errors
/// Returns an error if any GitHub API call fails.
pub async fn ci_failures(token: &str, repos: &[(String, String)]) -> Result<Vec<CiFailure>> {
    if repos.is_empty() {
        return Ok(vec![]);
    }

    let futures: Vec<_> = repos
        .iter()
        .map(|(repo, lookback)| fetch_repo_ci_failures(token, repo, lookback))
        .collect();

    let results = futures::future::join_all(futures).await;

    let mut failures = Vec::new();
    for result in results {
        failures.extend(result?);
    }
    Ok(failures)
}

async fn fetch_repo_ci_failures(token: &str, repo: &str, lookback: &str) -> Result<Vec<CiFailure>> {
    let cutoff = parse_cutoff(lookback)
        .with_context(|| format!("invalid lookback '{lookback}' for repo {repo}"))?;

    let (repo_info, runs_response) =
        tokio::join!(get_repo_info(token, repo), get_completed_runs(token, repo));
    let repo_info = repo_info?;
    let runs_response = runs_response?;

    let filtered = filter_runs(
        runs_response.workflow_runs,
        &repo_info.default_branch,
        cutoff,
    );

    filtered
        .into_iter()
        .map(|run| {
            let (owner, name) = repo
                .split_once('/')
                .ok_or_else(|| anyhow::anyhow!("expected 'owner/repo', got: {repo}"))?;
            Ok(CiFailure {
                repo: RepoSlug::new(owner, name),
                workflow_name: run.name,
                conclusion: run.conclusion.unwrap_or_default(),
                age_hours: age_hours(&run.created_at),
                url: run.html_url,
            })
        })
        .collect()
}

fn parse_cutoff(lookback: &str) -> Result<chrono::DateTime<Utc>> {
    let duration = humantime::parse_duration(lookback)
        .with_context(|| format!("failed to parse duration: {lookback}"))?;
    let secs = duration.as_secs();
    let delta = chrono::Duration::seconds(secs.try_into().unwrap_or(i64::MAX));
    Ok(Utc::now() - delta)
}

fn age_hours(created_at: &str) -> u64 {
    let Ok(created) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return 0;
    };
    let hours = (Utc::now() - created.to_utc()).num_hours();
    hours.max(0).cast_unsigned()
}

/// Keeps only the latest run per workflow file path that failed on the default branch
/// within the lookback window.
fn filter_runs(
    runs: Vec<WorkflowRun>,
    default_branch: &str,
    cutoff: chrono::DateTime<Utc>,
) -> Vec<WorkflowRun> {
    use std::collections::HashMap;

    let mut latest: HashMap<String, WorkflowRun> = HashMap::new();

    for run in runs {
        let Some(ref conclusion) = run.conclusion else {
            continue;
        };
        if !FAILING_CONCLUSIONS.contains(&conclusion.as_str()) {
            continue;
        }
        if run.head_branch != default_branch {
            continue;
        }
        let Ok(created) = chrono::DateTime::parse_from_rfc3339(&run.created_at) else {
            continue;
        };
        if created.to_utc() < cutoff {
            continue;
        }
        latest
            .entry(run.path.clone())
            .and_modify(|existing| {
                if run.created_at > existing.created_at {
                    *existing = run.clone();
                }
            })
            .or_insert(run);
    }

    latest.into_values().collect()
}

async fn get_repo_info(token: &str, repo: &str) -> Result<RepoInfo> {
    reqwest::Client::new()
        .get(format!("https://api.github.com/repos/{repo}"))
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
        .context("failed to parse repo info response")
}

async fn get_completed_runs(token: &str, repo: &str) -> Result<RunsResponse> {
    reqwest::Client::new()
        .get(format!("https://api.github.com/repos/{repo}/actions/runs"))
        .query(&[("status", "completed"), ("per_page", "100")])
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
        .context("failed to parse workflow runs response")
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
        let slug = repo_slug_from_url("https://api.github.com/repos/ooloth/hub")
            .expect("valid GitHub API URL");
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

    // ── filter_runs ───────────────────────────────────────────────────────────

    fn make_run(
        path: &str,
        name: &str,
        branch: &str,
        conclusion: Option<&str>,
        created_at: &str,
    ) -> WorkflowRun {
        WorkflowRun {
            path: path.into(),
            name: name.into(),
            head_branch: branch.into(),
            conclusion: conclusion.map(Into::into),
            created_at: created_at.into(),
            html_url: format!("https://github.com/runs/{path}"),
        }
    }

    fn recent() -> &'static str {
        "2099-01-01T00:00:00Z"
    }

    fn old() -> &'static str {
        "2000-01-01T00:00:00Z"
    }

    fn far_future_cutoff() -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339("2100-01-01T00:00:00Z")
            .unwrap()
            .to_utc()
    }

    fn past_cutoff() -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339("2001-01-01T00:00:00Z")
            .unwrap()
            .to_utc()
    }

    #[test]
    fn keeps_failing_run_on_default_branch_within_window() {
        let runs = vec![make_run(
            ".github/workflows/ci.yml",
            "CI",
            "main",
            Some("failure"),
            recent(),
        )];
        let result = filter_runs(runs, "main", past_cutoff());
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn drops_run_on_non_default_branch() {
        let runs = vec![make_run(
            ".github/workflows/ci.yml",
            "CI",
            "feat",
            Some("failure"),
            recent(),
        )];
        let result = filter_runs(runs, "main", past_cutoff());
        assert!(result.is_empty());
    }

    #[test]
    fn drops_run_outside_lookback_window() {
        let runs = vec![make_run(
            ".github/workflows/ci.yml",
            "CI",
            "main",
            Some("failure"),
            old(),
        )];
        let result = filter_runs(runs, "main", far_future_cutoff());
        assert!(result.is_empty());
    }

    #[test]
    fn drops_successful_run() {
        let runs = vec![make_run(
            ".github/workflows/ci.yml",
            "CI",
            "main",
            Some("success"),
            recent(),
        )];
        let result = filter_runs(runs, "main", past_cutoff());
        assert!(result.is_empty());
    }

    #[test]
    fn drops_cancelled_run() {
        let runs = vec![make_run(
            ".github/workflows/ci.yml",
            "CI",
            "main",
            Some("cancelled"),
            recent(),
        )];
        let result = filter_runs(runs, "main", past_cutoff());
        assert!(result.is_empty());
    }

    #[test]
    fn drops_run_with_no_conclusion() {
        let runs = vec![make_run(
            ".github/workflows/ci.yml",
            "CI",
            "main",
            None,
            recent(),
        )];
        let result = filter_runs(runs, "main", past_cutoff());
        assert!(result.is_empty());
    }

    #[test]
    fn keeps_latest_run_per_workflow_path() {
        let runs = vec![
            make_run(
                ".github/workflows/ci.yml",
                "CI",
                "main",
                Some("failure"),
                "2099-01-01T00:00:00Z",
            ),
            make_run(
                ".github/workflows/ci.yml",
                "CI",
                "main",
                Some("failure"),
                "2099-01-02T00:00:00Z",
            ),
        ];
        let result = filter_runs(runs, "main", past_cutoff());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].created_at, "2099-01-02T00:00:00Z");
    }

    #[test]
    fn keeps_one_entry_per_distinct_workflow_path() {
        let runs = vec![
            make_run(
                ".github/workflows/ci.yml",
                "CI",
                "main",
                Some("failure"),
                recent(),
            ),
            make_run(
                ".github/workflows/deploy.yml",
                "Deploy",
                "main",
                Some("timed_out"),
                recent(),
            ),
        ];
        let result = filter_runs(runs, "main", past_cutoff());
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn includes_all_failing_conclusions() {
        for conclusion in FAILING_CONCLUSIONS {
            let runs = vec![make_run(
                ".github/workflows/ci.yml",
                "CI",
                "main",
                Some(conclusion),
                recent(),
            )];
            let result = filter_runs(runs, "main", past_cutoff());
            assert_eq!(result.len(), 1, "expected {conclusion} to be kept");
        }
    }

    #[test]
    fn parse_cutoff_accepts_valid_durations() {
        assert!(parse_cutoff("24h").is_ok());
        assert!(parse_cutoff("1h").is_ok());
        assert!(parse_cutoff("7d").is_ok());
    }

    #[test]
    fn parse_cutoff_rejects_invalid_input() {
        assert!(parse_cutoff("not-a-duration").is_err());
    }
}
