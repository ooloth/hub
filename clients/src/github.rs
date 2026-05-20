use anyhow::{Context, Result};
use chrono::Utc;
use domain::{
    ChangedFile, CiFailure, CiStatus, GithubPrsRepo, Issue, PrKind, PullRequest, RepoSlug,
    ReviewDecision, Urgency, NEEDS_HUMAN_REVIEW_LABEL,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct ApiIssue {
    number: u64,
    title: String,
    html_url: String,
    user: ApiUser,
    created_at: String,
    #[serde(default)]
    labels: Vec<ApiLabel>,
    #[serde(default)]
    assignees: Vec<ApiAssignee>,
    #[serde(default)]
    pull_request: Option<PullRequestMarker>,
    body: Option<String>,
}

#[derive(Deserialize)]
struct ApiUser {
    login: String,
}

#[derive(Deserialize)]
struct ApiLabel {
    name: String,
}

#[derive(Deserialize)]
struct ApiAssignee {
    login: String,
}

#[derive(Deserialize)]
struct PullRequestMarker {}

// ── GraphQL (pull requests) ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GraphQlResponse {
    data: GraphQlData,
}

#[derive(Deserialize)]
struct GraphQlData {
    search: GraphQlSearch,
}

#[derive(Deserialize)]
struct GraphQlSearch {
    nodes: Vec<PrNode>,
}

#[derive(Deserialize)]
struct PrNode {
    number: u64,
    title: String,
    url: String,
    body: Option<String>,
    author: PrAuthor,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    #[serde(rename = "reviewDecision")]
    review_decision: Option<String>,
    reviews: ReviewCounts,
    repository: PrRepository,
    assignees: PrAssignees,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "baseRefName")]
    base_ref_name: String,
    #[serde(default)]
    commits: CommitConnection,
    #[serde(default)]
    files: FileConnection,
}

#[derive(Deserialize)]
struct PrAuthor {
    login: String,
}

#[derive(Deserialize)]
struct ReviewCounts {
    #[serde(rename = "totalCount")]
    total_count: u32,
}

#[derive(Deserialize)]
struct PrRepository {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

#[derive(Deserialize)]
struct PrAssignees {
    nodes: Vec<PrAssignee>,
}

#[derive(Deserialize)]
struct PrAssignee {
    login: String,
}

#[derive(Deserialize, Default)]
struct CommitConnection {
    nodes: Vec<CommitNode>,
}

#[derive(Deserialize)]
struct CommitNode {
    commit: CommitObject,
}

#[derive(Deserialize)]
struct CommitObject {
    #[serde(rename = "statusCheckRollup")]
    status_check_rollup: Option<StatusCheckRollup>,
}

#[derive(Deserialize)]
struct StatusCheckRollup {
    state: String,
}

#[derive(Deserialize, Default)]
struct FileConnection {
    #[serde(rename = "totalCount")]
    total_count: u32,
    nodes: Vec<FileNode>,
}

#[derive(Deserialize)]
struct FileNode {
    path: String,
    additions: u32,
    deletions: u32,
}

#[derive(Deserialize)]
struct PrFileEntry {
    filename: String,
    #[serde(default)]
    patch: Option<String>,
}

fn age(created_at: &str) -> chrono::Duration {
    let Ok(created) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return chrono::Duration::zero();
    };
    let d = Utc::now() - created.to_utc();
    if d < chrono::Duration::zero() {
        chrono::Duration::zero()
    } else {
        d
    }
}

/// Returns PRs across the given repos where review has been requested from the authenticated user.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable or returns a non-2xx response.
pub async fn prs_awaiting_review(token: &str, repos: &[GithubPrsRepo]) -> Result<Vec<PullRequest>> {
    if repos.is_empty() {
        return Ok(vec![]);
    }
    let mut prs = nodes_to_prs(
        graphql_prs(token, "is:open is:pr review-requested:@me", repos).await?,
        Urgency::Medium,
        PrKind::ToReview,
        None,
        repos,
    )?;
    enrich_with_file_patches(token, &mut prs).await;
    Ok(prs)
}

/// Returns open non-draft PRs across the given repos authored by `github_username`,
/// excluding any PR assigned to someone other than `github_username`.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable or returns a non-2xx response.
pub async fn my_open_prs(
    token: &str,
    repos: &[GithubPrsRepo],
    github_username: &str,
) -> Result<Vec<PullRequest>> {
    if repos.is_empty() {
        return Ok(vec![]);
    }
    let mut prs = nodes_to_prs(
        graphql_prs(token, "is:open is:pr -is:draft author:@me", repos).await?,
        Urgency::High,
        PrKind::Mine,
        Some(github_username),
        repos,
    )?;
    enrich_with_file_patches(token, &mut prs).await;
    Ok(prs)
}

/// Returns open draft PRs across the given repos authored by `github_username`,
/// excluding any PR assigned to someone other than `github_username`.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable or returns a non-2xx response.
pub async fn my_draft_prs(
    token: &str,
    repos: &[GithubPrsRepo],
    github_username: &str,
) -> Result<Vec<PullRequest>> {
    if repos.is_empty() {
        return Ok(vec![]);
    }
    let mut prs = nodes_to_prs(
        graphql_prs(token, "is:open is:pr is:draft author:@me", repos).await?,
        Urgency::Medium,
        PrKind::MyDraft,
        Some(github_username),
        repos,
    )?;
    enrich_with_file_patches(token, &mut prs).await;
    Ok(prs)
}

fn nodes_to_prs(
    nodes: Vec<PrNode>,
    urgency: Urgency,
    kind: PrKind,
    owned_by: Option<&str>,
    repos: &[GithubPrsRepo],
) -> Result<Vec<PullRequest>> {
    nodes
        .into_iter()
        .filter(|node| match owned_by {
            Some(username) => {
                node.assignees.nodes.iter().all(|a| a.login == username)
                    || node.assignees.nodes.is_empty()
            }
            None => true,
        })
        .filter(|node| {
            let excluded = repos
                .iter()
                .find(|r| r.repo == node.repository.name_with_owner)
                .map(|r| r.exclude_authors.iter().any(|a| a == &node.author.login))
                .unwrap_or(false);
            !excluded
        })
        .map(|node| {
            let (owner, repo) =
                node.repository
                    .name_with_owner
                    .split_once('/')
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "expected 'owner/repo', got: {}",
                            node.repository.name_with_owner
                        )
                    })?;
            Ok(PullRequest {
                number: node.number,
                title: node.title,
                repo: RepoSlug::new(owner, repo),
                url: node.url,
                body: node.body,
                ci_status: node
                    .commits
                    .nodes
                    .first()
                    .and_then(|c| c.commit.status_check_rollup.as_ref())
                    .and_then(|r| parse_ci_state(&r.state)),
                total_changed_files: node.files.total_count,
                changed_files: node
                    .files
                    .nodes
                    .into_iter()
                    .map(|f| ChangedFile {
                        path: f.path,
                        additions: f.additions,
                        deletions: f.deletions,
                        patch: None,
                    })
                    .collect(),
                age: age(&node.created_at),
                urgency,
                kind: if node.is_draft { PrKind::MyDraft } else { kind },
                author: node.author.login,
                review_decision: parse_review_decision(node.review_decision.as_deref()),
                review_count: node.reviews.total_count,
                head_branch: node.head_ref_name,
                base_branch: node.base_ref_name,
            })
        })
        .collect()
}

async fn fetch_file_patches(
    token: &str,
    repo: &str,
    number: u64,
) -> std::collections::HashMap<String, Option<String>> {
    let result: Result<Vec<PrFileEntry>> = async {
        let entries: Vec<PrFileEntry> = reqwest::Client::new()
            .get(format!(
                "https://api.github.com/repos/{repo}/pulls/{number}/files"
            ))
            .query(&[("per_page", "100")])
            .bearer_auth(token)
            .header("User-Agent", "hub-cli")
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .context("failed to reach GitHub API for PR files")?
            .error_for_status()
            .context("GitHub API error fetching PR files")?
            .json()
            .await
            .context("failed to parse PR files response")?;
        Ok(entries)
    }
    .await;
    result
        .unwrap_or_default()
        .into_iter()
        .map(|e| (e.filename, e.patch))
        .collect()
}

async fn enrich_with_file_patches(token: &str, prs: &mut [PullRequest]) {
    let futures: Vec<_> = prs
        .iter()
        .map(|pr| {
            let token = token.to_string();
            let repo = pr.repo.to_string();
            let number = pr.number;
            async move { fetch_file_patches(&token, &repo, number).await }
        })
        .collect();
    let all_patches = futures::future::join_all(futures).await;
    for (pr, patches) in prs.iter_mut().zip(all_patches) {
        for file in pr.changed_files.iter_mut() {
            if let Some(patch) = patches.get(&file.path) {
                file.patch = patch.clone();
            }
        }
    }
}

fn parse_ci_state(state: &str) -> Option<CiStatus> {
    match state {
        "SUCCESS" => Some(CiStatus::Success),
        "FAILURE" | "ERROR" => Some(CiStatus::Failure),
        "PENDING" | "EXPECTED" => Some(CiStatus::Pending),
        "NEUTRAL" => Some(CiStatus::Neutral),
        _ => None,
    }
}

fn parse_review_decision(s: Option<&str>) -> Option<ReviewDecision> {
    match s? {
        "APPROVED" => Some(ReviewDecision::Approved),
        "CHANGES_REQUESTED" => Some(ReviewDecision::ChangesRequested),
        _ => None,
    }
}

async fn graphql_prs(token: &str, base: &str, repos: &[GithubPrsRepo]) -> Result<Vec<PrNode>> {
    let repo_filters = repos
        .iter()
        .map(|r| format!("repo:{}", r.repo))
        .collect::<Vec<_>>()
        .join(" ");
    let q = format!("{base} {repo_filters}");
    let query = format!(
        r#"{{ search(query: "{q}", type: ISSUE, first: 100) {{ nodes {{ ... on PullRequest {{
            number title url body
            author {{ login }}
            createdAt isDraft reviewDecision
            headRefName baseRefName
            reviews {{ totalCount }}
            repository {{ nameWithOwner }}
            assignees(first: 10) {{ nodes {{ login }} }}
            commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state }} }} }} }}
            files(first: 100) {{ totalCount nodes {{ path additions deletions }} }}
        }} }} }} }}"#
    );
    let response: GraphQlResponse = reqwest::Client::new()
        .post("https://api.github.com/graphql")
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await
        .context("failed to reach GitHub GraphQL API")?
        .error_for_status()
        .context("GitHub GraphQL API returned an error")?
        .json()
        .await
        .context("failed to parse GitHub GraphQL response")?;
    Ok(response.data.search.nodes)
}

/// Returns all open issues across the given repos. Issues assigned to `username`
/// or labelled `status:needs-human-review` receive `Urgency::Medium`; all others
/// receive `Urgency::Low`. Pull requests returned by the API are filtered out.
///
/// Fetches all pages per repo (up to 100 per page) in parallel across repos.
///
/// # Errors
/// Returns an error if any GitHub API call fails.
pub async fn issues(token: &str, repos: &[String], username: &str) -> Result<Vec<Issue>> {
    if repos.is_empty() {
        return Ok(vec![]);
    }

    let futures: Vec<_> = repos
        .iter()
        .map(|repo| {
            let token = token.to_string();
            let repo = repo.clone();
            async move {
                let (owner, name) = repo
                    .split_once('/')
                    .ok_or_else(|| anyhow::anyhow!("invalid repo slug: {repo}"))?;
                let raw = fetch_repo_issues(&token, owner, name).await?;
                Ok::<(RepoSlug, Vec<ApiIssue>), anyhow::Error>((RepoSlug::new(owner, name), raw))
            }
        })
        .collect();

    let results = futures::future::join_all(futures).await;
    let mut issues = Vec::new();
    for result in results {
        let (repo_slug, raw_items) = result?;
        for item in raw_items {
            if let Some(issue) = to_domain_issue(item, repo_slug.clone(), username) {
                issues.push(issue);
            }
        }
    }
    Ok(issues)
}

async fn fetch_repo_issues(token: &str, owner: &str, repo: &str) -> Result<Vec<ApiIssue>> {
    let mut all = Vec::new();
    let mut page = 1u32;
    loop {
        let page_s = page.to_string();
        let items: Vec<ApiIssue> = reqwest::Client::new()
            .get(format!(
                "https://api.github.com/repos/{owner}/{repo}/issues"
            ))
            .query(&[
                ("state", "open"),
                ("per_page", "100"),
                ("page", page_s.as_str()),
            ])
            .bearer_auth(token)
            .header("User-Agent", "hub-cli")
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .with_context(|| format!("failed to reach GitHub API for {owner}/{repo}"))?
            .error_for_status()
            .with_context(|| format!("GitHub API error for {owner}/{repo}"))?
            .json()
            .await
            .with_context(|| {
                format!("failed to parse GitHub issues response for {owner}/{repo}")
            })?;
        let done = !has_next_page(items.len(), 100);
        all.extend(items);
        if done {
            break;
        }
        page += 1;
    }
    Ok(all)
}

fn to_domain_issue(item: ApiIssue, repo: RepoSlug, username: &str) -> Option<Issue> {
    if item.pull_request.is_some() {
        return None;
    }
    let assignees: Vec<String> = item.assignees.into_iter().map(|a| a.login).collect();
    let labels: Vec<String> = item.labels.into_iter().map(|l| l.name).collect();
    Some(Issue {
        number: item.number,
        title: item.title,
        repo,
        url: item.html_url,
        author: item.user.login,
        age: age(&item.created_at),
        urgency: classify_urgency(&assignees, &labels, username),
        labels,
        body: item.body,
    })
}

fn classify_urgency(assignees: &[String], labels: &[String], username: &str) -> Urgency {
    if assignees.iter().any(|a| a == username)
        || labels.iter().any(|l| l == NEEDS_HUMAN_REVIEW_LABEL)
    {
        Urgency::Medium
    } else {
        Urgency::Low
    }
}

fn has_next_page(returned: usize, per_page: usize) -> bool {
    returned == per_page
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
    reqwest::Client::new()
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

/// Dismisses an issue as won't fix: replaces its labels, optionally posts a
/// reason comment, then closes the issue with `state_reason: not_planned`.
///
/// Steps run sequentially; if any step fails the error identifies which one.
pub async fn dismiss_issue(
    token: &str,
    repo: &str,
    number: u64,
    reason: &str,
    labels: &[String],
) -> Result<()> {
    let client = reqwest::Client::new();

    client
        .put(format!(
            "https://api.github.com/repos/{repo}/issues/{number}/labels"
        ))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .json(&serde_json::json!({ "labels": labels }))
        .send()
        .await
        .with_context(|| format!("failed to reach GitHub API setting labels on {repo}#{number}"))?
        .error_for_status()
        .with_context(|| format!("GitHub API error setting labels on {repo}#{number}"))?;

    if !reason.is_empty() {
        client
            .post(format!(
                "https://api.github.com/repos/{repo}/issues/{number}/comments"
            ))
            .bearer_auth(token)
            .header("User-Agent", "hub-cli")
            .header("Accept", "application/vnd.github.v3+json")
            .json(&serde_json::json!({ "body": reason }))
            .send()
            .await
            .with_context(|| {
                format!("failed to reach GitHub API posting comment on {repo}#{number}")
            })?
            .error_for_status()
            .with_context(|| format!("GitHub API error posting comment on {repo}#{number}"))?;
    }

    client
        .patch(format!(
            "https://api.github.com/repos/{repo}/issues/{number}"
        ))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .json(&serde_json::json!({ "state": "closed", "state_reason": "not_planned" }))
        .send()
        .await
        .with_context(|| format!("failed to reach GitHub API closing {repo}#{number}"))?
        .error_for_status()
        .with_context(|| format!("GitHub API error closing {repo}#{number}"))?;

    Ok(())
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

    // ── classify_urgency ──────────────────────────────────────────────────────

    #[rstest::rstest]
    #[case(vec!["alice"], vec![], "alice", Urgency::Medium)]
    #[case(vec![], vec!["status:needs-human-review"], "alice", Urgency::Medium)]
    #[case(vec!["alice"], vec!["status:needs-human-review"], "alice", Urgency::Medium)]
    #[case(vec![], vec![], "alice", Urgency::Low)]
    #[case(vec!["bob"], vec![], "alice", Urgency::Low)]
    #[case(vec!["bob"], vec!["other-label"], "alice", Urgency::Low)]
    fn classify_urgency_cases(
        #[case] assignees: Vec<&str>,
        #[case] labels: Vec<&str>,
        #[case] username: &str,
        #[case] expected: Urgency,
    ) {
        let assignees: Vec<String> = assignees.into_iter().map(str::to_string).collect();
        let labels: Vec<String> = labels.into_iter().map(str::to_string).collect();
        assert_eq!(classify_urgency(&assignees, &labels, username), expected);
    }

    // ── has_next_page ─────────────────────────────────────────────────────────

    #[test]
    fn has_next_page_true_when_full_page_returned() {
        assert!(has_next_page(100, 100));
    }

    #[test]
    fn has_next_page_false_when_short_page_returned() {
        assert!(!has_next_page(99, 100));
    }

    #[test]
    fn has_next_page_false_when_empty_page_returned() {
        assert!(!has_next_page(0, 100));
    }

    // ── to_domain_issue ───────────────────────────────────────────────────────

    fn make_api_issue(pull_request: Option<PullRequestMarker>) -> ApiIssue {
        ApiIssue {
            number: 7,
            title: "Fix the thing".to_string(),
            html_url: "https://github.com/owner/repo/issues/7".to_string(),
            user: ApiUser {
                login: "bob".to_string(),
            },
            created_at: "2024-01-01T00:00:00Z".to_string(),
            labels: vec![ApiLabel {
                name: "bug".to_string(),
            }],
            assignees: vec![ApiAssignee {
                login: "alice".to_string(),
            }],
            pull_request,
            body: Some("This is the issue body.".to_string()),
        }
    }

    #[test]
    fn to_domain_issue_returns_none_for_pull_request() {
        let item = make_api_issue(Some(PullRequestMarker {}));
        assert!(to_domain_issue(item, RepoSlug::new("owner", "repo"), "alice").is_none());
    }

    #[test]
    fn to_domain_issue_maps_fields_correctly() {
        let item = make_api_issue(None);
        let issue = to_domain_issue(item, RepoSlug::new("owner", "repo"), "alice").unwrap();
        assert_eq!(issue.number, 7);
        assert_eq!(issue.title, "Fix the thing");
        assert_eq!(issue.repo.to_string(), "owner/repo");
        assert_eq!(issue.url, "https://github.com/owner/repo/issues/7");
        assert_eq!(issue.author, "bob");
        assert_eq!(issue.labels, vec!["bug"]);
        assert_eq!(issue.urgency, Urgency::Medium); // assigned to alice
        assert_eq!(issue.body, Some("This is the issue body.".to_string()));
    }

    #[test]
    fn to_domain_issue_body_none_when_absent() {
        let mut item = make_api_issue(None);
        item.body = None;
        let issue = to_domain_issue(item, RepoSlug::new("owner", "repo"), "alice").unwrap();
        assert_eq!(issue.body, None);
    }

    #[test]
    fn to_domain_issue_unassigned_no_label_is_low_urgency() {
        let mut item = make_api_issue(None);
        item.assignees = vec![];
        item.labels = vec![];
        let issue = to_domain_issue(item, RepoSlug::new("owner", "repo"), "alice").unwrap();
        assert_eq!(issue.urgency, Urgency::Low);
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

    // ── parse_ci_state ────────────────────────────────────────────────────────

    #[rstest::rstest]
    #[case("SUCCESS", Some(CiStatus::Success))]
    #[case("FAILURE", Some(CiStatus::Failure))]
    #[case("ERROR", Some(CiStatus::Failure))]
    #[case("PENDING", Some(CiStatus::Pending))]
    #[case("EXPECTED", Some(CiStatus::Pending))]
    #[case("NEUTRAL", Some(CiStatus::Neutral))]
    #[case("", None)]
    #[case("UNKNOWN", None)]
    fn parse_ci_state_cases(#[case] input: &str, #[case] expected: Option<CiStatus>) {
        assert_eq!(parse_ci_state(input), expected);
    }

    // ── nodes_to_prs (assignee and author filtering) ─────────────────────────

    fn make_node(assignee_logins: Vec<&str>) -> PrNode {
        make_node_with_author(assignee_logins, "alice", "owner/repo")
    }

    fn make_node_with_author(assignee_logins: Vec<&str>, author: &str, repo: &str) -> PrNode {
        PrNode {
            number: 1,
            title: "title".into(),
            url: "https://github.com/owner/repo/pull/1".into(),
            body: None,
            author: PrAuthor {
                login: author.into(),
            },
            created_at: "2024-01-01T00:00:00Z".into(),
            is_draft: false,
            review_decision: None,
            reviews: ReviewCounts { total_count: 0 },
            repository: PrRepository {
                name_with_owner: repo.into(),
            },
            assignees: PrAssignees {
                nodes: assignee_logins
                    .into_iter()
                    .map(|l| PrAssignee { login: l.into() })
                    .collect(),
            },
            head_ref_name: "feat/test".into(),
            base_ref_name: "main".into(),
            commits: CommitConnection::default(),
            files: FileConnection::default(),
        }
    }

    fn pr_repo(repo: &str, exclude_authors: Vec<&str>) -> GithubPrsRepo {
        GithubPrsRepo {
            repo: repo.into(),
            exclude_authors: exclude_authors.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn nodes_to_prs_includes_unassigned_pr() {
        let result = nodes_to_prs(
            vec![make_node(vec![])],
            Urgency::High,
            PrKind::Mine,
            Some("alice"),
            &[],
        )
        .unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn nodes_to_prs_includes_pr_assigned_to_user() {
        let result = nodes_to_prs(
            vec![make_node(vec!["alice"])],
            Urgency::High,
            PrKind::Mine,
            Some("alice"),
            &[],
        )
        .unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn nodes_to_prs_excludes_pr_assigned_to_someone_else() {
        let result = nodes_to_prs(
            vec![make_node(vec!["bob"])],
            Urgency::High,
            PrKind::Mine,
            Some("alice"),
            &[],
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn nodes_to_prs_excludes_pr_from_repo_excluded_author() {
        let node = make_node_with_author(vec![], "dependabot-preview[bot]", "owner/a");
        let result = nodes_to_prs(
            vec![node],
            Urgency::Medium,
            PrKind::ToReview,
            None,
            &[pr_repo("owner/a", vec!["dependabot-preview[bot]"])],
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn nodes_to_prs_keeps_pr_when_author_excluded_only_in_different_repo() {
        let node = make_node_with_author(vec![], "dependabot", "owner/b");
        let result = nodes_to_prs(
            vec![node],
            Urgency::Medium,
            PrKind::ToReview,
            None,
            &[
                pr_repo("owner/a", vec!["dependabot"]),
                pr_repo("owner/b", vec![]),
            ],
        )
        .unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn nodes_to_prs_keeps_pr_from_non_excluded_author_when_exclude_list_present() {
        let node = make_node_with_author(vec![], "alice", "owner/a");
        let result = nodes_to_prs(
            vec![node],
            Urgency::Medium,
            PrKind::ToReview,
            None,
            &[pr_repo("owner/a", vec!["dependabot"])],
        )
        .unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn nodes_to_prs_with_empty_exclude_lists_passes_all_prs() {
        let nodes = vec![
            make_node_with_author(vec![], "alice", "owner/a"),
            make_node_with_author(vec![], "bob", "owner/a"),
        ];
        let result = nodes_to_prs(
            nodes,
            Urgency::Medium,
            PrKind::ToReview,
            None,
            &[pr_repo("owner/a", vec![])],
        )
        .unwrap();
        assert_eq!(result.len(), 2);
    }

    // ── parse_review_decision ─────────────────────────────────────────────────

    #[test]
    fn parse_review_decision_approved() {
        assert_eq!(
            parse_review_decision(Some("APPROVED")),
            Some(ReviewDecision::Approved)
        );
    }

    #[test]
    fn parse_review_decision_changes_requested() {
        assert_eq!(
            parse_review_decision(Some("CHANGES_REQUESTED")),
            Some(ReviewDecision::ChangesRequested)
        );
    }

    #[test]
    fn parse_review_decision_review_required_maps_to_none() {
        assert_eq!(parse_review_decision(Some("REVIEW_REQUIRED")), None);
    }

    #[test]
    fn parse_review_decision_none_maps_to_none() {
        assert_eq!(parse_review_decision(None), None);
    }
}
