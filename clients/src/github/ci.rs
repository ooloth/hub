use anyhow::{Context, Result};
use chrono::Utc;
use domain::{CiFailure, RepoSlug, Urgency};
use serde::Deserialize;

use super::age;

// ── API types ─────────────────────────────────────────────────────────────────

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
    id: u64,
    path: String,
    name: String,
    head_branch: String,
    conclusion: Option<String>,
    created_at: String,
    html_url: String,
}

#[derive(Deserialize)]
struct JobsResponse {
    jobs: Vec<Job>,
}

#[derive(Deserialize)]
struct Job {
    id: u64,
    name: String,
    conclusion: Option<String>,
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct Step {
    name: String,
    conclusion: Option<String>,
}

#[derive(Deserialize)]
struct Annotation {
    annotation_level: String,
    message: String,
}

const FAILING_CONCLUSIONS: &[&str] =
    &["failure", "timed_out", "startup_failure", "action_required"];

// ── Public fetch function ─────────────────────────────────────────────────────

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

// ── Internal helpers ──────────────────────────────────────────────────────────

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

    let futures: Vec<_> = filtered
        .into_iter()
        .map(|run| {
            let token = token.to_string();
            let repo = repo.to_string();
            async move { enrich_run(&token, &repo, run).await }
        })
        .collect();

    futures::future::join_all(futures)
        .await
        .into_iter()
        .collect()
}

async fn enrich_run(token: &str, repo: &str, run: WorkflowRun) -> Result<CiFailure> {
    let (owner, name) = repo
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("expected 'owner/repo', got: {repo}"))?;

    let (job_name, step_name, job_id) = match get_failed_job(token, repo, run.id).await {
        Some((j, s, id)) => (Some(j), s, Some(id)),
        None => (None, None, None),
    };

    let error = match job_id {
        Some(id) => get_first_error_annotation(token, repo, id).await,
        None => None,
    };

    Ok(CiFailure {
        repo: RepoSlug::new(owner, name),
        workflow_name: run.name,
        job_name,
        step_name,
        error,
        age: age(&run.created_at),
        urgency: Urgency::High,
        url: run.html_url,
    })
}

/// Returns the first failed job's name, its first failed step name (if any), and its id.
async fn get_failed_job(
    token: &str,
    repo: &str,
    run_id: u64,
) -> Option<(String, Option<String>, u64)> {
    let response: JobsResponse = reqwest::Client::new()
        .get(format!(
            "https://api.github.com/repos/{repo}/actions/runs/{run_id}/jobs"
        ))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;

    let job = response
        .jobs
        .into_iter()
        .find(|j| j.conclusion.as_deref() == Some("failure"))?;

    let step_name = job
        .steps
        .iter()
        .find(|s| s.conclusion.as_deref() == Some("failure"))
        .map(|s| s.name.clone());

    Some((job.name, step_name, job.id))
}

/// Returns the first line of the first failure-level annotation for a job.
async fn get_first_error_annotation(token: &str, repo: &str, job_id: u64) -> Option<String> {
    let annotations: Vec<Annotation> = reqwest::Client::new()
        .get(format!(
            "https://api.github.com/repos/{repo}/check-runs/{job_id}/annotations"
        ))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;

    annotations
        .into_iter()
        .find(|a| a.annotation_level == "failure")
        .and_then(|a| a.message.lines().next().map(str::to_string))
        .filter(|s| !s.is_empty())
}

fn parse_cutoff(lookback: &str) -> Result<chrono::DateTime<Utc>> {
    let duration = humantime::parse_duration(lookback)
        .with_context(|| format!("failed to parse duration: {lookback}"))?;
    let secs = duration.as_secs();
    let delta = chrono::Duration::seconds(secs.try_into().unwrap_or(i64::MAX));
    Ok(Utc::now() - delta)
}

/// Keeps only workflows whose latest completed run on the default branch (within the
/// lookback window) has a failing conclusion. A subsequent success clears the failure.
fn filter_runs(
    runs: Vec<WorkflowRun>,
    default_branch: &str,
    cutoff: chrono::DateTime<Utc>,
) -> Vec<WorkflowRun> {
    use std::collections::HashMap;

    let mut latest: HashMap<String, WorkflowRun> = HashMap::new();

    for run in runs {
        if run.conclusion.is_none() {
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

    latest
        .into_values()
        .filter(|run| {
            run.conclusion
                .as_deref()
                .is_some_and(|c| FAILING_CONCLUSIONS.contains(&c))
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_run(
        path: &str,
        name: &str,
        branch: &str,
        conclusion: Option<&str>,
        created_at: &str,
    ) -> WorkflowRun {
        WorkflowRun {
            id: 0,
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
    fn success_after_failure_clears_the_failure() {
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
                Some("success"),
                "2099-01-02T00:00:00Z",
            ),
        ];
        let result = filter_runs(runs, "main", past_cutoff());
        assert!(result.is_empty());
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
