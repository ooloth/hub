use anyhow::{Context, Result};
use chrono::Utc;
use domain::{
    ChangedFile, CiFailure, CiStatus, GithubPrsRepo, Issue, IssueState, MergeBlocker, PrKind,
    PrState, PullRequest, RepoSlug, ReviewComment, ReviewDecision, ReviewThread, Urgency,
    NEEDS_HUMAN_REVIEW_LABEL,
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
    reviews: ReviewConnection,
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
    #[serde(default)]
    #[serde(rename = "reviewThreads")]
    review_threads: ReviewThreadConnection,
    #[serde(default)]
    comments: PrCommentConnection,
    #[serde(default)]
    #[serde(rename = "mergeStateStatus")]
    merge_state_status: Option<String>,
}

#[derive(Deserialize)]
struct PrAuthor {
    login: String,
}

#[derive(Deserialize, Default)]
struct ReviewConnection {
    nodes: Vec<ReviewStateNode>,
}

#[derive(Deserialize)]
struct ReviewStateNode {
    state: String,
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

#[derive(Deserialize, Default)]
struct ReviewThreadConnection {
    nodes: Vec<ReviewThreadNode>,
}

#[derive(Deserialize)]
struct ReviewThreadNode {
    #[serde(rename = "isResolved")]
    is_resolved: bool,
    path: String,
    line: Option<u32>,
    #[serde(rename = "diffSide")]
    diff_side: String,
    comments: ReviewCommentConnection,
}

#[derive(Deserialize, Default)]
struct ReviewCommentConnection {
    nodes: Vec<ReviewCommentNode>,
}

#[derive(Deserialize)]
struct ReviewCommentNode {
    author: ReviewCommentAuthor,
    #[serde(rename = "createdAt")]
    created_at: String,
    body: String,
}

#[derive(Deserialize)]
struct ReviewCommentAuthor {
    login: String,
}

#[derive(Deserialize, Default)]
struct PrCommentConnection {
    nodes: Vec<PrCommentNode>,
}

#[derive(Deserialize)]
struct PrCommentNode {
    author: ReviewCommentAuthor,
    #[serde(rename = "createdAt")]
    created_at: String,
    body: String,
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
        Urgency::High,
        PrKind::ToReview,
        None,
        repos,
    )?;
    enrich_with_file_patches(token, &mut prs).await;
    Ok(prs)
}

/// Returns open non-draft PRs across the given repos not authored by the authenticated user
/// and not where their review was explicitly requested.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable or returns a non-2xx response.
pub async fn external_prs(token: &str, repos: &[GithubPrsRepo]) -> Result<Vec<PullRequest>> {
    if repos.is_empty() {
        return Ok(vec![]);
    }
    let mut prs = nodes_to_prs(
        graphql_prs(
            token,
            "is:open is:pr -is:draft -author:@me -review-requested:@me",
            repos,
        )
        .await?,
        Urgency::Medium,
        PrKind::External,
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
            let approval_count = node
                .reviews
                .nodes
                .iter()
                .filter(|r| r.state == "APPROVED")
                .count() as u32;
            let thread_comment_count: usize = node
                .review_threads
                .nodes
                .iter()
                .map(|t| t.comments.nodes.len())
                .sum();
            let comment_count = (thread_comment_count + node.comments.nodes.len()) as u32;
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
                review_threads: node
                    .review_threads
                    .nodes
                    .into_iter()
                    .filter(|t| !t.is_resolved && t.diff_side == "RIGHT")
                    .map(|t| ReviewThread {
                        path: t.path,
                        line: t.line,
                        comments: t
                            .comments
                            .nodes
                            .into_iter()
                            .map(|c| ReviewComment {
                                author: c.author.login,
                                age: age(&c.created_at),
                                body: c.body,
                            })
                            .collect(),
                    })
                    .collect(),
                pr_comments: node
                    .comments
                    .nodes
                    .into_iter()
                    .map(|c| ReviewComment {
                        author: c.author.login,
                        age: age(&c.created_at),
                        body: c.body,
                    })
                    .collect(),
                age: age(&node.created_at),
                urgency,
                kind: if node.is_draft { PrKind::MyDraft } else { kind },
                author: node.author.login,
                review_decision: parse_review_decision(node.review_decision.as_deref()),
                approval_count,
                comment_count,
                head_branch: node.head_ref_name,
                base_branch: node.base_ref_name,
                merge_blocker: parse_merge_blocker(node.merge_state_status.as_deref()),
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

fn parse_pr_state(s: Option<&str>) -> Option<PrState> {
    match s? {
        "OPEN" => Some(PrState::Open),
        "MERGED" => Some(PrState::Merged),
        "CLOSED" => Some(PrState::Closed),
        _ => None,
    }
}

/// Maps a GitHub issue's `state` + `stateReason` pair to the normalised
/// `IssueState`. Returns `None` when `state` is absent or unrecognised.
///
/// Mapping:
/// - `OPEN` → `Open` (stateReason ignored — only set on closed issues)
/// - `CLOSED` + `NOT_PLANNED` → `Cancelled`
/// - `CLOSED` + anything else (COMPLETED, REOPENED, null) → `Completed`
fn parse_issue_state(state: Option<&str>, state_reason: Option<&str>) -> Option<IssueState> {
    match state? {
        "OPEN" => Some(IssueState::Open),
        "CLOSED" => Some(match state_reason {
            Some("NOT_PLANNED") => IssueState::Cancelled,
            _ => IssueState::Completed,
        }),
        _ => None,
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

fn parse_merge_blocker(s: Option<&str>) -> Option<MergeBlocker> {
    match s? {
        "DIRTY" => Some(MergeBlocker::Conflict),
        "BEHIND" => Some(MergeBlocker::Behind),
        "BLOCKED" => Some(MergeBlocker::Blocked),
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
            createdAt isDraft reviewDecision mergeStateStatus
            headRefName baseRefName
            reviews(first: 50) {{ nodes {{ state }} }}
            repository {{ nameWithOwner }}
            assignees(first: 10) {{ nodes {{ login }} }}
            commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state }} }} }} }}
            files(first: 100) {{ totalCount nodes {{ path additions deletions }} }}
            reviewThreads(first: 50) {{ nodes {{ isResolved path line diffSide
                comments(first: 10) {{ nodes {{ author {{ login }} createdAt body }} }}
            }} }}
            comments(first: 50) {{ nodes {{ author {{ login }} createdAt body }} }}
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

// ── PR state lookup (fold-back) ───────────────────────────────────────────────

/// Returns the current state of each pull request identified by `(repo, number)`.
///
/// Uses a single batched GraphQL query. PRs not found in the response are
/// omitted from the result map — a map miss means "no action" for the caller.
///
/// # Errors
/// Returns an error only for network failures, HTTP errors, or a malformed
/// response body. A PR GitHub can't find produces a null node; those are
/// silently skipped rather than failing the whole batch.
pub async fn pr_states(
    token: &str,
    prs: &[(RepoSlug, u64)],
) -> Result<std::collections::HashMap<(RepoSlug, u64), PrState>> {
    pr_states_with_base("https://api.github.com", token, prs).await
}

/// Internal entry point that accepts a custom base URL for testing with wiremock.
async fn pr_states_with_base(
    base: &str,
    token: &str,
    prs: &[(RepoSlug, u64)],
) -> Result<std::collections::HashMap<(RepoSlug, u64), PrState>> {
    if prs.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    // Group PRs by repo so we query each repository node once.
    // We use a Vec (not a HashMap) to preserve insertion order — the order
    // determines the alias indices (r0, r1, …) and we need to reconstruct
    // that same mapping when we read the response.
    let mut repo_map: Vec<(RepoSlug, Vec<u64>)> = Vec::new();
    for (repo, number) in prs {
        match repo_map.iter_mut().find(|(r, _)| r == repo) {
            Some((_, numbers)) => numbers.push(*number),
            None => repo_map.push((repo.clone(), vec![*number])),
        }
    }

    // Build a batched GraphQL query using dynamic aliases.
    //
    // Why aliases, not search?
    // All existing PR queries use `search(query: "is:open is:pr …")`, which
    // only returns open PRs. Merged and closed PRs vanish from those results,
    // so detecting a merge via absence is unreliable (a transient API error
    // also looks like "gone"). Instead we query each PR directly by identity:
    //   repository(owner: "ooloth", name: "hub") {
    //     pullRequest(number: 42) { state }
    //   }
    //
    // Why dynamic aliases?
    // GraphQL requires every field returned to have a unique name within its
    // parent object. Querying multiple repositories — or multiple PRs within
    // a repo — in a single document would produce duplicate `repository` /
    // `pullRequest` keys without aliases. Aliases rename each field for the
    // duration of that query:
    //
    //   r0: repository(owner: "ooloth", name: "hub") {
    //     p0: pullRequest(number: 42) { state }
    //     p1: pullRequest(number: 99) { state }
    //   }
    //   r1: repository(owner: "other", name: "repo") {
    //     p0: pullRequest(number: 7) { state }
    //   }
    //
    // The `r{i}` / `p{j}` scheme encodes the position of each (repo, number)
    // pair, so we can map the response back to the original inputs.
    let mut repo_parts = Vec::new();
    for (i, (repo, numbers)) in repo_map.iter().enumerate() {
        let slug = repo.to_string();
        let (owner, name) = slug
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("invalid repo slug: {repo}"))?;

        let pr_fields: Vec<String> = numbers
            .iter()
            .enumerate()
            .map(|(j, number)| format!("p{j}: pullRequest(number: {number}) {{ state }}"))
            .collect();

        repo_parts.push(format!(
            r#"r{i}: repository(owner: "{owner}", name: "{name}") {{ {} }}"#,
            pr_fields.join(" ")
        ));
    }

    let query = format!("{{ {} }}", repo_parts.join(" "));

    // serde_json::Value is used here (not a fixed struct) because the field
    // names in the response are the dynamic aliases we generated above — they
    // aren't known at compile time, so no static schema can describe them.
    let body: serde_json::Value = reqwest::Client::new()
        .post(format!("{base}/graphql"))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await
        .context("failed to reach GitHub GraphQL API for PR states")?
        .error_for_status()
        .context("GitHub GraphQL API returned an error for PR states")?
        .json()
        .await
        .context("failed to parse GitHub GraphQL response for PR states")?;

    // GraphQL can return HTTP 200 with an "errors" array for partial or total
    // failures. Treat any top-level errors as a hard failure — the caller
    // (fold-back) will skip this refresh cycle rather than misclassify tasks.
    if let Some(errors) = body.get("errors") {
        anyhow::bail!("GitHub GraphQL returned errors for PR states: {errors}");
    }

    let data = body
        .get("data")
        .context("GitHub GraphQL response missing 'data' field")?;

    // Walk the same r{i}/p{j} tree we built when constructing the query.
    // A null repo node means the repo was not found; a null PR node means the
    // PR was not found. Both are skipped — a map miss is the caller's cue to
    // take no action on that task.
    let mut result = std::collections::HashMap::new();
    for (i, (repo, numbers)) in repo_map.iter().enumerate() {
        let repo_node = match data.get(format!("r{i}")) {
            Some(v) if !v.is_null() => v,
            _ => continue,
        };
        for (j, number) in numbers.iter().enumerate() {
            let pr_node = match repo_node.get(format!("p{j}")) {
                Some(v) if !v.is_null() => v,
                _ => continue,
            };
            if let Some(state) = parse_pr_state(pr_node.get("state").and_then(|s| s.as_str())) {
                result.insert((repo.clone(), *number), state);
            }
        }
    }

    Ok(result)
}

// ── Issue state lookup (fold-back) ───────────────────────────────────────────

/// Returns the current state of each issue identified by `(repo, number)`.
///
/// Uses a single batched GraphQL query with dynamic aliases — the same
/// approach as `pr_states`. Issues not found in the response are omitted from
/// the result map — a map miss means "no action" for the caller.
///
/// # Errors
/// Returns an error only for network failures, HTTP errors, or a malformed
/// response body. An issue GitHub can't find produces a null node; those are
/// silently skipped rather than failing the whole batch.
pub async fn issue_states(
    token: &str,
    issues: &[(RepoSlug, u64)],
) -> Result<std::collections::HashMap<(RepoSlug, u64), IssueState>> {
    issue_states_with_base("https://api.github.com", token, issues).await
}

/// Internal entry point that accepts a custom base URL for testing with wiremock.
async fn issue_states_with_base(
    base: &str,
    token: &str,
    issues: &[(RepoSlug, u64)],
) -> Result<std::collections::HashMap<(RepoSlug, u64), IssueState>> {
    if issues.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    // Group issues by repo to query each repository node once.
    let mut repo_map: Vec<(RepoSlug, Vec<u64>)> = Vec::new();
    for (repo, number) in issues {
        match repo_map.iter_mut().find(|(r, _)| r == repo) {
            Some((_, numbers)) => numbers.push(*number),
            None => repo_map.push((repo.clone(), vec![*number])),
        }
    }

    // Build a batched GraphQL query using dynamic aliases (r{i}/i{j}).
    // Each issue node requests both `state` and `stateReason` so we can
    // distinguish completed from not-planned closes.
    let mut repo_parts = Vec::new();
    for (i, (repo, numbers)) in repo_map.iter().enumerate() {
        let slug = repo.to_string();
        let (owner, name) = slug
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("invalid repo slug: {repo}"))?;

        let issue_fields: Vec<String> = numbers
            .iter()
            .enumerate()
            .map(|(j, number)| format!("i{j}: issue(number: {number}) {{ state stateReason }}"))
            .collect();

        repo_parts.push(format!(
            r#"r{i}: repository(owner: "{owner}", name: "{name}") {{ {} }}"#,
            issue_fields.join(" ")
        ));
    }

    let query = format!("{{ {} }}", repo_parts.join(" "));

    let body: serde_json::Value = reqwest::Client::new()
        .post(format!("{base}/graphql"))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await
        .context("failed to reach GitHub GraphQL API for issue states")?
        .error_for_status()
        .context("GitHub GraphQL API returned an error for issue states")?
        .json()
        .await
        .context("failed to parse GitHub GraphQL response for issue states")?;

    if let Some(errors) = body.get("errors") {
        anyhow::bail!("GitHub GraphQL returned errors for issue states: {errors}");
    }

    let data = body
        .get("data")
        .context("GitHub GraphQL response missing 'data' field")?;

    let mut result = std::collections::HashMap::new();
    for (i, (repo, numbers)) in repo_map.iter().enumerate() {
        let repo_node = match data.get(format!("r{i}")) {
            Some(v) if !v.is_null() => v,
            _ => continue,
        };
        for (j, number) in numbers.iter().enumerate() {
            let issue_node = match repo_node.get(format!("i{j}")) {
                Some(v) if !v.is_null() => v,
                _ => continue,
            };
            let state = issue_node.get("state").and_then(|s| s.as_str());
            let state_reason = issue_node.get("stateReason").and_then(|s| s.as_str());
            if let Some(s) = parse_issue_state(state, state_reason) {
                result.insert((repo.clone(), *number), s);
            }
        }
    }

    Ok(result)
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
/// If any step after the label PUT fails, the original labels are restored
/// before returning the error. If the restore also fails, both errors are
/// reported in the error chain.
pub async fn dismiss_issue(
    token: &str,
    repo: &str,
    number: u64,
    reason: &str,
    labels: &[String],
) -> Result<()> {
    dismiss_issue_with_base(
        "https://api.github.com",
        token,
        repo,
        number,
        reason,
        labels,
    )
    .await
}

async fn dismiss_issue_with_base(
    base: &str,
    token: &str,
    repo: &str,
    number: u64,
    reason: &str,
    labels: &[String],
) -> Result<()> {
    let client = reqwest::Client::new();

    let original_labels = fetch_issue_labels(&client, base, token, repo, number).await?;

    client
        .put(format!("{base}/repos/{repo}/issues/{number}/labels"))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .json(&serde_json::json!({ "labels": labels }))
        .send()
        .await
        .with_context(|| format!("failed to reach GitHub API setting labels on {repo}#{number}"))?
        .error_for_status()
        .with_context(|| format!("GitHub API error setting labels on {repo}#{number}"))?;

    if let Err(e) = complete_dismissal(&client, base, token, repo, number, reason).await {
        let rollback = client
            .put(format!("{base}/repos/{repo}/issues/{number}/labels"))
            .bearer_auth(token)
            .header("User-Agent", "hub-cli")
            .header("Accept", "application/vnd.github.v3+json")
            .json(&serde_json::json!({ "labels": original_labels }))
            .send()
            .await
            .and_then(|r| r.error_for_status());
        return match rollback {
            Ok(_) => Err(e),
            Err(rb_err) => Err(e.context(format!("label rollback also failed: {rb_err}"))),
        };
    }

    Ok(())
}

async fn fetch_issue_labels(
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

async fn complete_dismissal(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    repo: &str,
    number: u64,
    reason: &str,
) -> Result<()> {
    if !reason.is_empty() {
        client
            .post(format!("{base}/repos/{repo}/issues/{number}/comments"))
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
        .patch(format!("{base}/repos/{repo}/issues/{number}"))
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

    // ── parse_pr_state ────────────────────────────────────────────────────────

    #[rstest::rstest]
    #[case(Some("OPEN"), Some(PrState::Open))]
    #[case(Some("MERGED"), Some(PrState::Merged))]
    #[case(Some("CLOSED"), Some(PrState::Closed))]
    #[case(Some("UNKNOWN"), None)]
    #[case(Some(""), None)]
    #[case(None, None)]
    fn parse_pr_state_cases(#[case] input: Option<&str>, #[case] expected: Option<PrState>) {
        assert_eq!(parse_pr_state(input), expected);
    }

    // ── pr_states_with_base ───────────────────────────────────────────────────

    fn graphql_pr_response(alias_states: &[(&str, &str, &[(&str, &str)])]) -> serde_json::Value {
        // Builds the response JSON that GitHub would return for a batched alias query.
        // alias_states: [(repo_alias, _, [(pr_alias, state), ...])]
        let mut data = serde_json::Map::new();
        for (repo_alias, _, prs) in alias_states {
            let mut repo_obj = serde_json::Map::new();
            for (pr_alias, state) in *prs {
                repo_obj.insert(pr_alias.to_string(), serde_json::json!({ "state": state }));
            }
            data.insert(repo_alias.to_string(), serde_json::Value::Object(repo_obj));
        }
        serde_json::json!({ "data": data })
    }

    #[tokio::test]
    async fn pr_states_returns_merged_for_merged_pr() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_pr_response(&[(
                    "r0",
                    "owner/repo",
                    &[("p0", "MERGED")],
                )])),
            )
            .mount(&server)
            .await;

        let result = pr_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 42)],
        )
        .await
        .unwrap();

        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 42)],
            PrState::Merged
        );
    }

    #[tokio::test]
    async fn pr_states_returns_closed_for_closed_pr() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_pr_response(&[(
                    "r0",
                    "owner/repo",
                    &[("p0", "CLOSED")],
                )])),
            )
            .mount(&server)
            .await;

        let result = pr_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 7)],
        )
        .await
        .unwrap();

        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 7)],
            PrState::Closed
        );
    }

    #[tokio::test]
    async fn pr_states_returns_open_for_open_pr() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_pr_response(&[(
                    "r0",
                    "owner/repo",
                    &[("p0", "OPEN")],
                )])),
            )
            .mount(&server)
            .await;

        let result = pr_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 1)],
        )
        .await
        .unwrap();

        assert_eq!(result[&(RepoSlug::new("owner", "repo"), 1)], PrState::Open);
    }

    #[tokio::test]
    async fn pr_states_batches_multiple_prs_in_same_repo() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_pr_response(&[(
                    "r0",
                    "owner/repo",
                    &[("p0", "MERGED"), ("p1", "CLOSED")],
                )])),
            )
            .mount(&server)
            .await;

        let result = pr_states_with_base(
            &server.uri(),
            "token",
            &[
                (RepoSlug::new("owner", "repo"), 10),
                (RepoSlug::new("owner", "repo"), 11),
            ],
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 10)],
            PrState::Merged
        );
        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 11)],
            PrState::Closed
        );
    }

    #[tokio::test]
    async fn pr_states_null_pr_node_is_skipped() {
        let server = MockServer::start().await;
        // GitHub returns null for a pullRequest node when the PR doesn't exist.
        let body = serde_json::json!({
            "data": { "r0": { "p0": serde_json::Value::Null } }
        });
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let result = pr_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 999)],
        )
        .await
        .unwrap();

        assert!(result.is_empty(), "expected map miss for not-found PR");
    }

    #[tokio::test]
    async fn pr_states_graphql_errors_field_returns_err() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "errors": [{ "message": "something went wrong" }]
        });
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let result = pr_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 42)],
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn pr_states_empty_input_makes_no_http_call() {
        // No mock registered — any HTTP call would panic the test.
        let server = MockServer::start().await;
        let result = pr_states_with_base(&server.uri(), "token", &[])
            .await
            .unwrap();
        assert!(result.is_empty());
    }

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
            reviews: ReviewConnection::default(),
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
            review_threads: ReviewThreadConnection::default(),
            comments: PrCommentConnection::default(),
            merge_state_status: None,
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

    // ── parse_merge_blocker ───────────────────────────────────────────────────

    #[rstest::rstest]
    #[case(Some("DIRTY"), Some(MergeBlocker::Conflict))]
    #[case(Some("BEHIND"), Some(MergeBlocker::Behind))]
    #[case(Some("BLOCKED"), Some(MergeBlocker::Blocked))]
    #[case(Some("CLEAN"), None)]
    #[case(Some("UNSTABLE"), None)]
    #[case(Some("UNKNOWN"), None)]
    #[case(Some(""), None)]
    #[case(None, None)]
    fn parse_merge_blocker_cases(
        #[case] input: Option<&str>,
        #[case] expected: Option<MergeBlocker>,
    ) {
        assert_eq!(parse_merge_blocker(input), expected);
    }

    // ── dismiss_issue ─────────────────────────────────────────────────────────

    use wiremock::{matchers, Mock, MockServer, ResponseTemplate};

    fn label_json(names: &[&str]) -> serde_json::Value {
        serde_json::json!(names
            .iter()
            .map(|n| serde_json::json!({ "name": n }))
            .collect::<Vec<_>>())
    }

    async fn mock_get_labels(server: &MockServer, number: u64, labels: &[&str]) {
        Mock::given(matchers::method("GET"))
            .and(matchers::path(format!(
                "/repos/owner/repo/issues/{number}/labels"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(label_json(labels)))
            .expect(1)
            .mount(server)
            .await;
    }

    async fn mock_get_labels_fail(server: &MockServer, number: u64) {
        Mock::given(matchers::method("GET"))
            .and(matchers::path(format!(
                "/repos/owner/repo/issues/{number}/labels"
            )))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(server)
            .await;
    }

    // Matches PUT /labels by the body it sends, so initial PUT and rollback PUT
    // can coexist in the same mock server without ambiguity.
    async fn mock_put_labels(server: &MockServer, number: u64, body_labels: &[&str], status: u16) {
        Mock::given(matchers::method("PUT"))
            .and(matchers::path(format!(
                "/repos/owner/repo/issues/{number}/labels"
            )))
            .and(matchers::body_json(
                serde_json::json!({ "labels": body_labels }),
            ))
            .respond_with(ResponseTemplate::new(status).set_body_json(label_json(&[])))
            .expect(1)
            .mount(server)
            .await;
    }

    async fn mock_post_comment(server: &MockServer, number: u64, status: u16) {
        Mock::given(matchers::method("POST"))
            .and(matchers::path(format!(
                "/repos/owner/repo/issues/{number}/comments"
            )))
            .respond_with(ResponseTemplate::new(status).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(server)
            .await;
    }

    async fn mock_patch_close(server: &MockServer, number: u64, status: u16) {
        Mock::given(matchers::method("PATCH"))
            .and(matchers::path(format!("/repos/owner/repo/issues/{number}")))
            .respond_with(ResponseTemplate::new(status).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn dismiss_issue_succeeds_without_reason() {
        let server = MockServer::start().await;
        mock_get_labels(&server, 7, &["bug"]).await;
        mock_put_labels(&server, 7, &["wontfix"], 200).await;
        mock_patch_close(&server, 7, 200).await;

        let result = dismiss_issue_with_base(
            &server.uri(),
            "token",
            "owner/repo",
            7,
            "",
            &["wontfix".to_string()],
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dismiss_issue_succeeds_with_reason() {
        let server = MockServer::start().await;
        mock_get_labels(&server, 7, &["bug"]).await;
        mock_put_labels(&server, 7, &["wontfix"], 200).await;
        mock_post_comment(&server, 7, 201).await;
        mock_patch_close(&server, 7, 200).await;

        let result = dismiss_issue_with_base(
            &server.uri(),
            "token",
            "owner/repo",
            7,
            "not planned",
            &["wontfix".to_string()],
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dismiss_issue_rolls_back_labels_when_comment_fails() {
        let server = MockServer::start().await;
        mock_get_labels(&server, 7, &["bug"]).await;
        mock_put_labels(&server, 7, &["wontfix"], 200).await;
        mock_post_comment(&server, 7, 500).await;
        mock_put_labels(&server, 7, &["bug"], 200).await; // rollback restores original

        let result = dismiss_issue_with_base(
            &server.uri(),
            "token",
            "owner/repo",
            7,
            "reason",
            &["wontfix".to_string()],
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn dismiss_issue_rolls_back_labels_when_close_fails() {
        let server = MockServer::start().await;
        mock_get_labels(&server, 7, &["bug"]).await;
        mock_put_labels(&server, 7, &["wontfix"], 200).await;
        mock_post_comment(&server, 7, 201).await;
        mock_patch_close(&server, 7, 500).await;
        mock_put_labels(&server, 7, &["bug"], 200).await; // rollback restores original

        let result = dismiss_issue_with_base(
            &server.uri(),
            "token",
            "owner/repo",
            7,
            "reason",
            &["wontfix".to_string()],
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn dismiss_issue_makes_no_writes_when_label_fetch_fails() {
        let server = MockServer::start().await;
        mock_get_labels_fail(&server, 7).await;

        let result = dismiss_issue_with_base(
            &server.uri(),
            "token",
            "owner/repo",
            7,
            "reason",
            &["wontfix".to_string()],
        )
        .await;

        assert!(result.is_err());
        // wiremock's expect(1) on the GET verifies it ran; no PUT/POST/PATCH mocks
        // are registered, so any unexpected write would produce an error the test
        // would need to handle — but the function aborts before reaching them.
    }

    #[tokio::test]
    async fn dismiss_issue_surfaces_both_errors_when_rollback_also_fails() {
        let server = MockServer::start().await;
        mock_get_labels(&server, 7, &["bug"]).await;
        mock_put_labels(&server, 7, &["wontfix"], 200).await;
        mock_patch_close(&server, 7, 500).await;
        mock_put_labels(&server, 7, &["bug"], 500).await; // rollback also fails

        let result = dismiss_issue_with_base(
            &server.uri(),
            "token",
            "owner/repo",
            7,
            "",
            &["wontfix".to_string()],
        )
        .await;

        let err = result.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("rollback"), "expected 'rollback' in: {msg}");
    }

    // ── parse_issue_state ─────────────────────────────────────────────────────

    #[rstest::rstest]
    #[case(Some("OPEN"), None, Some(IssueState::Open))]
    #[case(Some("OPEN"), Some("COMPLETED"), Some(IssueState::Open))]
    #[case(Some("CLOSED"), Some("COMPLETED"), Some(IssueState::Completed))]
    #[case(Some("CLOSED"), Some("NOT_PLANNED"), Some(IssueState::Cancelled))]
    #[case(Some("CLOSED"), Some("REOPENED"), Some(IssueState::Completed))]
    #[case(Some("CLOSED"), None, Some(IssueState::Completed))]
    #[case(Some("UNKNOWN"), None, None)]
    #[case(None, None, None)]
    fn parse_issue_state_cases(
        #[case] state: Option<&str>,
        #[case] state_reason: Option<&str>,
        #[case] expected: Option<IssueState>,
    ) {
        assert_eq!(parse_issue_state(state, state_reason), expected);
    }

    // ── issue_states_with_base ────────────────────────────────────────────────

    fn graphql_issue_response(
        alias_issues: &[(&str, &[(&str, &str, Option<&str>)])],
    ) -> serde_json::Value {
        let mut data = serde_json::Map::new();
        for (repo_alias, issues) in alias_issues {
            let mut repo_obj = serde_json::Map::new();
            for (issue_alias, state, state_reason) in *issues {
                let mut node = serde_json::Map::new();
                node.insert(
                    "state".to_string(),
                    serde_json::Value::String(state.to_string()),
                );
                node.insert(
                    "stateReason".to_string(),
                    match state_reason {
                        Some(r) => serde_json::Value::String(r.to_string()),
                        None => serde_json::Value::Null,
                    },
                );
                repo_obj.insert(issue_alias.to_string(), serde_json::Value::Object(node));
            }
            data.insert(repo_alias.to_string(), serde_json::Value::Object(repo_obj));
        }
        serde_json::json!({ "data": data })
    }

    #[tokio::test]
    async fn issue_states_returns_completed_for_closed_completed_issue() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_issue_response(&[(
                    "r0",
                    &[("i0", "CLOSED", Some("COMPLETED"))],
                )])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 42)],
        )
        .await
        .unwrap();

        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 42)],
            IssueState::Completed
        );
    }

    #[tokio::test]
    async fn issue_states_returns_cancelled_for_not_planned_issue() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_issue_response(&[(
                    "r0",
                    &[("i0", "CLOSED", Some("NOT_PLANNED"))],
                )])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 7)],
        )
        .await
        .unwrap();

        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 7)],
            IssueState::Cancelled
        );
    }

    #[tokio::test]
    async fn issue_states_returns_open_for_open_issue() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(graphql_issue_response(&[("r0", &[("i0", "OPEN", None)])])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 1)],
        )
        .await
        .unwrap();

        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 1)],
            IssueState::Open
        );
    }

    #[tokio::test]
    async fn issue_states_completed_fallback_for_null_state_reason() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(graphql_issue_response(&[("r0", &[("i0", "CLOSED", None)])])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 3)],
        )
        .await
        .unwrap();

        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 3)],
            IssueState::Completed
        );
    }

    #[tokio::test]
    async fn issue_states_null_issue_node_is_skipped() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "data": { "r0": { "i0": serde_json::Value::Null } }
        });
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 999)],
        )
        .await
        .unwrap();

        assert!(result.is_empty(), "expected map miss for not-found issue");
    }

    #[tokio::test]
    async fn issue_states_batches_multiple_issues_across_repos() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_issue_response(&[
                    (
                        "r0",
                        &[("i0", "CLOSED", Some("COMPLETED")), ("i1", "OPEN", None)],
                    ),
                    ("r1", &[("i0", "CLOSED", Some("NOT_PLANNED"))]),
                ])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[
                (RepoSlug::new("owner", "repo-a"), 10),
                (RepoSlug::new("owner", "repo-a"), 11),
                (RepoSlug::new("owner", "repo-b"), 5),
            ],
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(
            result[&(RepoSlug::new("owner", "repo-a"), 10)],
            IssueState::Completed
        );
        assert_eq!(
            result[&(RepoSlug::new("owner", "repo-a"), 11)],
            IssueState::Open
        );
        assert_eq!(
            result[&(RepoSlug::new("owner", "repo-b"), 5)],
            IssueState::Cancelled
        );
    }
}
