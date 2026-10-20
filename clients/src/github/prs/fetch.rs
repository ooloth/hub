use std::collections::HashMap;

use anyhow::{Context, Result};
use domain::pr_id::PrId;
use domain::review_waiting::{CommitOid, Feedback, LastPush, ReviewWaiting};
use domain::{
    ChangedFile, CiStatus, GithubPrsRepo, MergeBlocker, MergeReadiness, PrKind, PullRequest,
    RepoSlug, ReviewComment, ReviewDecision, ReviewThread, Urgency,
};
use serde::Deserialize;

use super::super::age;
use super::last_push::last_push_with_base;
use super::review_activity::{review_activity, RecentCommentConnection, RecentReviewConnection};

const GITHUB_API: &str = "https://api.github.com";

// ── GraphQL response types ────────────────────────────────────────────────────

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
    /// Asked for my own pull requests only, by [`RECENT_ACTIVITY`].
    #[serde(default)]
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
    /// `None` when the branch's repository, such as a fork, was deleted, and for pull requests
    /// that are not mine, which do not ask for it.
    #[serde(default)]
    #[serde(rename = "headRepository")]
    head_repository: Option<PrRepository>,
    #[serde(default)]
    #[serde(rename = "recentReviews")]
    recent_reviews: RecentReviewConnection,
    #[serde(default)]
    #[serde(rename = "recentComments")]
    recent_comments: RecentCommentConnection,
}

/// What my own pull requests also ask for: their newest reviews and comments, which decide whether
/// a review waits on me however many there are, and where their head was pushed.
const RECENT_ACTIVITY: &str = "headRefOid headRepository { nameWithOwner }
            recentReviews: reviews(last: 100) { nodes { state author { __typename login } submittedAt body comments { totalCount } } }
            recentComments: comments(last: 100) { nodes { author { login } createdAt } }";

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
    /// `None` if the reviewer's account was deleted.
    author: Option<PrAuthor>,
    /// `None` for `PENDING` reviews, which haven't been submitted yet.
    #[serde(rename = "submittedAt")]
    submitted_at: Option<String>,
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

// ── Public fetch functions ────────────────────────────────────────────────────

/// Returns PRs across the given repos where review has been requested from the authenticated user.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable or returns a non-2xx response.
pub async fn prs_awaiting_review(token: &str, repos: &[GithubPrsRepo]) -> Result<Vec<PullRequest>> {
    if repos.is_empty() {
        return Ok(vec![]);
    }

    let mut prs = nodes_to_prs(
        graphql_prs(token, "is:open is:pr review-requested:@me", "", repos).await?,
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
            "",
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

/// My open pull requests, and each one whose last push could not be read.
#[derive(Debug)]
pub struct MyOpenPrs {
    /// The pull requests. One whose last push could not be read has
    /// [`ReviewWaiting::PushUnknown`].
    pub prs: Vec<PullRequest>,
    /// Why each last push that was needed could not be read.
    pub last_push_failures: Vec<(PrId, anyhow::Error)>,
}

/// Returns open non-draft PRs across the given repos authored by `github_username`,
/// excluding any PR assigned to someone other than `github_username`, each with whether a
/// review waits on me.
///
/// My last push is looked up only for a pull request with a review I have not replied to, alongside
/// the requests for changed files.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable or returns a non-2xx response. A last push
/// that cannot be read is reported in [`MyOpenPrs::last_push_failures`] instead.
pub async fn my_open_prs(
    token: &str,
    repos: &[GithubPrsRepo],
    github_username: &str,
) -> Result<MyOpenPrs> {
    if repos.is_empty() {
        return Ok(MyOpenPrs {
            prs: vec![],
            last_push_failures: vec![],
        });
    }

    let nodes = graphql_prs(
        token,
        "is:open is:pr -is:draft author:@me",
        RECENT_ACTIVITY,
        repos,
    )
    .await?;
    let heads = heads(&nodes);
    let mut prs = nodes_to_prs(
        nodes,
        Urgency::High,
        PrKind::Mine,
        Some(github_username),
        repos,
    )?;

    let pending = pending_pushes(&prs, &heads);
    let (looked_up, ()) = tokio::join!(
        look_up_pushes(GITHUB_API, token, pending),
        enrich_with_file_patches(token, &mut prs)
    );
    let last_push_failures = apply_pushes(&mut prs, looked_up);

    Ok(MyOpenPrs {
        prs,
        last_push_failures,
    })
}

// ── Last pushes ───────────────────────────────────────────────────────────────

/// Where a pull request's head is: the repository its branch is in, and the commit at its head.
#[derive(Clone, Debug)]
struct Head {
    repo: Option<String>,
    oid: String,
}

fn heads(nodes: &[PrNode]) -> HashMap<(String, u64), Head> {
    nodes
        .iter()
        .map(|node| {
            (
                (node.repository.name_with_owner.clone(), node.number),
                Head {
                    repo: node
                        .head_repository
                        .as_ref()
                        .map(|r| r.name_with_owner.clone()),
                    oid: node.head_ref_oid.clone(),
                },
            )
        })
        .collect()
}

/// A pull request whose unreplied feedback waits on my last push to settle.
#[derive(Debug)]
struct PendingPush {
    index: usize,
    pr: PrId,
    feedback: Feedback,
    branch: String,
    head: Option<Head>,
}

fn pending_pushes(prs: &[PullRequest], heads: &HashMap<(String, u64), Head>) -> Vec<PendingPush> {
    prs.iter()
        .enumerate()
        .filter_map(|(index, pr)| match &pr.review_waiting {
            ReviewWaiting::PushUnknown(feedback) => Some(PendingPush {
                index,
                pr: PrId::of(pr),
                feedback: feedback.clone(),
                branch: pr.head_branch.clone(),
                head: heads.get(&(pr.repo.to_string(), pr.number)).cloned(),
            }),
            ReviewWaiting::NoReview
            | ReviewWaiting::Answered { .. }
            | ReviewWaiting::Waiting(_) => None,
        })
        .collect()
}

async fn look_up_pushes(
    base: &str,
    token: &str,
    pending: Vec<PendingPush>,
) -> Vec<(PendingPush, Result<LastPush>)> {
    futures::future::join_all(pending.into_iter().map(|pending| async move {
        let result = match &pending.head {
            Some(Head {
                repo: Some(repo),
                oid,
            }) if !oid.is_empty() => {
                last_push_with_base(base, token, repo, &pending.branch, &CommitOid::new(oid)).await
            }
            Some(Head { repo: None, .. }) => Err(anyhow::anyhow!("its head repository is gone")),
            Some(_) | None => Err(anyhow::anyhow!("GitHub did not say where its head is")),
        };
        (pending, result)
    }))
    .await
}

/// Settles each looked-up pull request, and returns why each one that failed could not be.
fn apply_pushes(
    prs: &mut [PullRequest],
    looked_up: Vec<(PendingPush, Result<LastPush>)>,
) -> Vec<(PrId, anyhow::Error)> {
    let mut failures = Vec::new();
    for (pending, result) in looked_up {
        let Some(pr) = prs.get_mut(pending.index) else {
            unreachable!("a lookup settles a pull request in the list it was found in");
        };
        assert_eq!(
            PrId::of(pr),
            pending.pr,
            "a lookup settles the pull request it was for"
        );
        match result {
            Ok(push) => pr.review_waiting = pending.feedback.against(Some(&push)),
            Err(error) => failures.push((
                pending.pr,
                error.context(format!("last push to {}", pending.branch)),
            )),
        }
    }
    failures
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
        graphql_prs(token, "is:open is:pr is:draft author:@me", "", repos).await?,
        Urgency::Medium,
        PrKind::MyDraft,
        Some(github_username),
        repos,
    )?;

    enrich_with_file_patches(token, &mut prs).await;

    Ok(prs)
}

// ── Conversion ────────────────────────────────────────────────────────────────

fn nodes_to_prs(
    nodes: Vec<PrNode>,
    urgency: Urgency,
    kind: PrKind,
    owned_by: Option<&str>,
    repos: &[GithubPrsRepo],
) -> Result<Vec<PullRequest>> {
    nodes
        .into_iter()
        .filter(|node| {
            owned_by.is_none_or(|username| {
                node.assignees.nodes.iter().all(|a| a.login == username)
                    || node.assignees.nodes.is_empty()
            })
        })
        .filter(|node| {
            let excluded = repos
                .iter()
                .find(|r| r.repo == node.repository.name_with_owner)
                .is_some_and(|r| r.exclude_authors.iter().any(|a| a == &node.author.login));

            !excluded
        })
        .map(|node| node_to_pr(node, urgency, kind, owned_by))
        .collect()
}

/// Reduces raw review nodes to each author's most recently submitted review.
///
/// `reviews(first: 50)` returns the oldest 50 reviews submitted, not one per
/// reviewer — a reviewer who re-reviews (e.g. requests changes, then later
/// approves) appears more than once. Counting raw nodes would count that
/// reviewer twice; GitHub's own review decision counts only the latest
/// review per reviewer. Nodes with no author (deleted account) or no
/// `submittedAt` (a `PENDING` review) are dropped.
fn latest_review_per_author(nodes: &[ReviewStateNode]) -> Vec<&ReviewStateNode> {
    let mut latest: std::collections::HashMap<
        &str,
        (&ReviewStateNode, chrono::DateTime<chrono::Utc>),
    > = std::collections::HashMap::new();

    for node in nodes {
        let Some(author) = &node.author else {
            continue;
        };
        let Some(submitted_at) = node.submitted_at.as_deref() else {
            continue;
        };
        let Ok(submitted) = chrono::DateTime::parse_from_rfc3339(submitted_at) else {
            continue;
        };
        let submitted = submitted.to_utc();

        let _ = latest
            .entry(author.login.as_str())
            .and_modify(|(existing_node, existing_time)| {
                if submitted > *existing_time {
                    *existing_node = node;
                    *existing_time = submitted;
                }
            })
            .or_insert((node, submitted));
    }

    latest.into_values().map(|(node, _)| node).collect()
}

/// Whether a review of `node` waits on me. `me` is my username, given for my own pull requests,
/// whose reviews are read. Until my last push is known, an unreplied one is
/// [`ReviewWaiting::PushUnknown`].
fn review_waiting(node: &PrNode, kind: PrKind, me: Option<&str>) -> ReviewWaiting {
    match (kind, me) {
        (PrKind::Mine, Some(me)) => {
            review_activity(&node.recent_reviews, &node.recent_comments, me)
                .latest_feedback()
                .settle_without_push()
                .unwrap_or_else(ReviewWaiting::PushUnknown)
        }
        _ => ReviewWaiting::NoReview,
    }
}

/// How many reviewers' latest review is in `state`.
fn count_latest_reviews(nodes: &[ReviewStateNode], state: &str) -> Result<u32> {
    let count = latest_review_per_author(nodes)
        .iter()
        .filter(|r| r.state == state)
        .count();
    u32::try_from(count).with_context(|| format!("{state} review count overflow"))
}

fn node_to_pr(
    node: PrNode,
    urgency: Urgency,
    kind: PrKind,
    me: Option<&str>,
) -> Result<PullRequest> {
    let (owner, repo) = node
        .repository
        .name_with_owner
        .split_once('/')
        .ok_or_else(|| {
            anyhow::anyhow!(
                "expected 'owner/repo', got: {}",
                node.repository.name_with_owner
            )
        })?;

    let approval_count = count_latest_reviews(&node.reviews.nodes, "APPROVED")?;
    let changes_requested_count = count_latest_reviews(&node.reviews.nodes, "CHANGES_REQUESTED")?;

    let thread_comment_count: usize = node
        .review_threads
        .nodes
        .iter()
        .map(|t| t.comments.nodes.len())
        .sum();

    let total_comments = thread_comment_count + node.comments.nodes.len();
    let comment_count = u32::try_from(total_comments).context("comment count overflow")?;

    let kind = if node.is_draft { PrKind::MyDraft } else { kind };
    let review_waiting = review_waiting(&node, kind, me);

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
        kind,
        author: node.author.login,
        review_decision: parse_review_decision(node.review_decision.as_deref()),
        approval_count,
        changes_requested_count,
        comment_count,
        head_branch: node.head_ref_name,
        base_branch: node.base_ref_name,
        merge_readiness: parse_merge_readiness(node.merge_state_status.as_deref()),
        review_waiting,
    })
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
        for file in &mut pr.changed_files {
            if let Some(patch) = patches.get(&file.path) {
                file.patch = patch.clone();
            }
        }
    }
}

// ── Parsers ───────────────────────────────────────────────────────────────────

fn parse_ci_state(state: &str) -> Option<CiStatus> {
    match state {
        "SUCCESS" => Some(CiStatus::Success),
        "FAILURE" | "ERROR" => Some(CiStatus::Failure),
        "PENDING" | "EXPECTED" => Some(CiStatus::Pending),
        "NEUTRAL" => Some(CiStatus::Neutral),
        _ => None,
    }
}

/// `UNSTABLE` is ready: only checks that are not required are failing, and GitHub allows the merge.
fn parse_merge_readiness(s: Option<&str>) -> MergeReadiness {
    match s {
        Some("DIRTY") => MergeReadiness::Blocked(MergeBlocker::Conflict),
        Some("BEHIND") => MergeReadiness::Blocked(MergeBlocker::Behind),
        Some("BLOCKED") => MergeReadiness::Blocked(MergeBlocker::Blocked),
        Some("CLEAN" | "UNSTABLE" | "HAS_HOOKS") => MergeReadiness::Ready,
        _ => MergeReadiness::NotYetKnown,
    }
}

fn parse_review_decision(s: Option<&str>) -> Option<ReviewDecision> {
    match s? {
        "APPROVED" => Some(ReviewDecision::Approved),
        "CHANGES_REQUESTED" => Some(ReviewDecision::ChangesRequested),
        _ => None,
    }
}

// ── GraphQL query ─────────────────────────────────────────────────────────────

/// `extra_fields` are asked for on every pull request found, beyond the fields every search asks for.
async fn graphql_prs(
    token: &str,
    base: &str,
    extra_fields: &str,
    repos: &[GithubPrsRepo],
) -> Result<Vec<PrNode>> {
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
            reviews(first: 50) {{ nodes {{ state author {{ login }} submittedAt }} }}
            repository {{ nameWithOwner }}
            assignees(first: 10) {{ nodes {{ login }} }}
            commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state }} }} }} }}
            files(first: 100) {{ totalCount nodes {{ path additions deletions }} }}
            reviewThreads(first: 50) {{ nodes {{ isResolved path line diffSide
                comments(first: 10) {{ nodes {{ author {{ login }} createdAt body }} }}
            }} }}
            comments(first: 50) {{ nodes {{ author {{ login }} createdAt body }} }}
            {extra_fields}
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

#[cfg(test)]
mod tests {
    use super::*;

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
            head_ref_oid: String::new(),
            head_repository: None,
            recent_reviews: RecentReviewConnection::default(),
            recent_comments: RecentCommentConnection::default(),
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

    // ── latest_review_per_author ──────────────────────────────────────────────

    fn review(state: &str, author: &str, submitted_at: &str) -> ReviewStateNode {
        ReviewStateNode {
            state: state.into(),
            author: Some(PrAuthor {
                login: author.into(),
            }),
            submitted_at: Some(submitted_at.into()),
        }
    }

    #[test]
    fn latest_review_per_author_keeps_one_review_per_distinct_author() {
        let nodes = vec![
            review("APPROVED", "alice", "2024-01-01T00:00:00Z"),
            review("CHANGES_REQUESTED", "bob", "2024-01-02T00:00:00Z"),
        ];
        assert_eq!(latest_review_per_author(&nodes).len(), 2);
    }

    #[test]
    fn latest_review_per_author_keeps_only_the_most_recent_review_when_author_reviews_twice() {
        let nodes = vec![
            review("CHANGES_REQUESTED", "alice", "2024-01-01T00:00:00Z"),
            review("APPROVED", "alice", "2024-01-02T00:00:00Z"),
        ];
        let latest = latest_review_per_author(&nodes);
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].state, "APPROVED");
    }

    #[test]
    fn latest_review_per_author_ignores_review_with_no_author() {
        let nodes = vec![ReviewStateNode {
            state: "APPROVED".into(),
            author: None,
            submitted_at: Some("2024-01-01T00:00:00Z".into()),
        }];
        assert!(latest_review_per_author(&nodes).is_empty());
    }

    #[test]
    fn latest_review_per_author_ignores_pending_review_with_no_submitted_at() {
        let nodes = vec![ReviewStateNode {
            state: "PENDING".into(),
            author: Some(PrAuthor {
                login: "alice".into(),
            }),
            submitted_at: None,
        }];
        assert!(latest_review_per_author(&nodes).is_empty());
    }

    // ── node_to_pr review counts ──────────────────────────────────────────────

    fn node_with_reviews(reviews: Vec<ReviewStateNode>) -> PrNode {
        let mut node = make_node(vec![]);
        node.reviews = ReviewConnection { nodes: reviews };
        node
    }

    #[test]
    fn node_to_pr_counts_one_approval_when_other_reviewer_only_commented() {
        // Regression test for the reported bug: a PR with 2 COMMENTED reviews
        // from one reviewer and 1 APPROVED review from another still counts
        // exactly 1 approval and 0 changes-requested.
        let node = node_with_reviews(vec![
            review("COMMENTED", "reviewer-a", "2024-01-01T00:00:00Z"),
            review("COMMENTED", "reviewer-a", "2024-01-01T01:00:00Z"),
            review("APPROVED", "reviewer-b", "2024-01-01T02:00:00Z"),
        ]);
        let pr = node_to_pr(node, Urgency::Medium, PrKind::ToReview, None).unwrap();
        assert_eq!(pr.approval_count, 1);
        assert_eq!(pr.changes_requested_count, 0);
    }

    #[test]
    fn node_to_pr_counts_approvals_and_changes_requested_from_different_reviewers() {
        let node = node_with_reviews(vec![
            review("APPROVED", "reviewer-a", "2024-01-01T00:00:00Z"),
            review("CHANGES_REQUESTED", "reviewer-b", "2024-01-01T01:00:00Z"),
        ]);
        let pr = node_to_pr(node, Urgency::Medium, PrKind::ToReview, None).unwrap();
        assert_eq!(pr.approval_count, 1);
        assert_eq!(pr.changes_requested_count, 1);
    }

    #[test]
    fn node_to_pr_counts_reviewer_who_re_reviews_only_once() {
        let node = node_with_reviews(vec![
            review("CHANGES_REQUESTED", "reviewer-a", "2024-01-01T00:00:00Z"),
            review("APPROVED", "reviewer-a", "2024-01-01T01:00:00Z"),
        ]);
        let pr = node_to_pr(node, Urgency::Medium, PrKind::ToReview, None).unwrap();
        assert_eq!(pr.approval_count, 1);
        assert_eq!(pr.changes_requested_count, 0);
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

    // ── parse_merge_readiness ─────────────────────────────────────────────────

    #[rstest::rstest]
    #[case::conflict(Some("DIRTY"), MergeReadiness::Blocked(MergeBlocker::Conflict))]
    #[case::behind(Some("BEHIND"), MergeReadiness::Blocked(MergeBlocker::Behind))]
    #[case::blocked(Some("BLOCKED"), MergeReadiness::Blocked(MergeBlocker::Blocked))]
    #[case::clean(Some("CLEAN"), MergeReadiness::Ready)]
    #[case::failing_checks_not_required(Some("UNSTABLE"), MergeReadiness::Ready)]
    #[case::hooks(Some("HAS_HOOKS"), MergeReadiness::Ready)]
    #[case::not_computed_yet(Some("UNKNOWN"), MergeReadiness::NotYetKnown)]
    #[case::draft(Some("DRAFT"), MergeReadiness::NotYetKnown)]
    #[case::unrecognised(Some("SOMETHING_NEW"), MergeReadiness::NotYetKnown)]
    #[case::empty(Some(""), MergeReadiness::NotYetKnown)]
    #[case::absent(None, MergeReadiness::NotYetKnown)]
    fn a_merge_state_is_ready_only_when_github_says_nothing_blocks_it(
        #[case] input: Option<&str>,
        #[case] expected: MergeReadiness,
    ) {
        assert_eq!(parse_merge_readiness(input), expected);
    }

    // ── my own pull requests: whether a review waits on me ───────────────────

    const HEAD: &str = "ffc50ad1e5b4a3c2d1f0e9d8c7b6a5f4e3d2c1b0";

    /// One of my pull requests, with Ada's review at `reviewed` and my top-level comments.
    fn my_node(number: u64, reviewed: Option<&str>, my_comments: &[&str]) -> PrNode {
        let mut node = make_node(vec![]);
        node.number = number;
        node.head_ref_oid = HEAD.to_string();
        node.head_ref_name = format!("claude/issue-{number}");
        node.head_repository = Some(PrRepository {
            name_with_owner: "owner/repo".to_string(),
        });
        node.recent_reviews = serde_json::from_value(serde_json::json!({ "nodes": reviewed
            .map(|at| vec![serde_json::json!({ "state": "CHANGES_REQUESTED",
                "author": { "__typename": "User", "login": "ada" },
                "submittedAt": at, "body": "please", "comments": { "totalCount": 1 } })])
            .unwrap_or_default() }))
        .unwrap();
        node.recent_comments = serde_json::from_value(serde_json::json!({ "nodes": my_comments
            .iter()
            .map(|at| serde_json::json!({ "author": { "login": "alice" }, "createdAt": at }))
            .collect::<Vec<_>>() }))
        .unwrap();
        node
    }

    fn mine(node: PrNode) -> PullRequest {
        node_to_pr(node, Urgency::High, PrKind::Mine, Some("alice")).unwrap()
    }

    #[test]
    fn an_unreplied_review_on_my_pull_request_waits_for_my_last_push() {
        let pr = mine(my_node(1, Some("2026-10-19T12:00:00Z"), &[]));

        assert!(
            matches!(pr.review_waiting, ReviewWaiting::PushUnknown(_)),
            "{:?}",
            pr.review_waiting
        );
    }

    #[test]
    fn a_review_i_replied_to_needs_no_push() {
        let pr = mine(my_node(
            1,
            Some("2026-10-19T12:00:00Z"),
            &["2026-10-19T13:00:00Z"],
        ));

        assert!(
            matches!(pr.review_waiting, ReviewWaiting::Answered { .. }),
            "{:?}",
            pr.review_waiting
        );
    }

    #[test]
    fn reviews_of_someone_elses_pull_request_are_not_read() {
        let node = my_node(1, Some("2026-10-19T12:00:00Z"), &[]);

        let pr = node_to_pr(node, Urgency::High, PrKind::ToReview, None).unwrap();

        assert_eq!(pr.review_waiting, ReviewWaiting::NoReview);
    }

    async fn activity_server(pushed: &str, expected_lookups: u64) -> wiremock::MockServer {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/repos/owner/repo/activity"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
                serde_json::json!([{ "after": HEAD, "timestamp": pushed, "activity_type": "push" }]),
            ))
            .expect(expected_lookups)
            .mount(&server)
            .await;
        server
    }

    async fn settle(
        server: &wiremock::MockServer,
        nodes: Vec<PrNode>,
    ) -> (Vec<PullRequest>, Vec<(PrId, anyhow::Error)>) {
        let heads = heads(&nodes);
        let mut prs: Vec<PullRequest> = nodes.into_iter().map(mine).collect();
        let pending = pending_pushes(&prs, &heads);
        let looked_up = look_up_pushes(&server.uri(), "token", pending).await;
        let failures = apply_pushes(&mut prs, looked_up);
        (prs, failures)
    }

    #[tokio::test]
    async fn no_push_is_looked_up_without_an_unreplied_review() {
        let server = activity_server("2026-10-19T00:00:00Z", 0).await;

        let (prs, failures) = settle(
            &server,
            vec![
                my_node(1, None, &[]),
                my_node(2, Some("2026-10-19T12:00:00Z"), &["2026-10-19T13:00:00Z"]),
            ],
        )
        .await;

        assert_eq!(prs.len(), 2);
        assert!(failures.is_empty());
    }

    #[tokio::test]
    async fn a_review_after_my_last_push_waits_on_me() {
        let server = activity_server("2026-10-19T11:00:00Z", 1).await;

        let (prs, failures) =
            settle(&server, vec![my_node(1, Some("2026-10-19T12:00:00Z"), &[])]).await;

        assert!(failures.is_empty());
        assert!(
            matches!(&prs[0].review_waiting, ReviewWaiting::Waiting(f) if f.by() == "ada"),
            "{:?}",
            prs[0].review_waiting
        );
    }

    #[tokio::test]
    async fn a_push_after_the_review_answers_it() {
        let server = activity_server("2026-10-19T13:00:00Z", 1).await;

        let (prs, _) = settle(&server, vec![my_node(1, Some("2026-10-19T12:00:00Z"), &[])]).await;

        assert!(
            matches!(prs[0].review_waiting, ReviewWaiting::Answered { .. }),
            "{:?}",
            prs[0].review_waiting
        );
    }

    #[tokio::test]
    async fn a_last_push_that_cannot_be_read_stays_unknown_and_is_reported() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let (prs, failures) =
            settle(&server, vec![my_node(7, Some("2026-10-19T12:00:00Z"), &[])]).await;

        assert!(matches!(
            prs[0].review_waiting,
            ReviewWaiting::PushUnknown(_)
        ));
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].0.to_string(), "owner/repo#7");
        assert!(
            format!("{:#}", failures[0].1).contains("claude/issue-7"),
            "{:#}",
            failures[0].1
        );
    }

    #[tokio::test]
    async fn a_pull_request_whose_head_repository_is_gone_has_no_known_push() {
        let server = activity_server("2026-10-19T11:00:00Z", 0).await;
        let mut node = my_node(1, Some("2026-10-19T12:00:00Z"), &[]);
        node.head_repository = None;

        let (prs, failures) = settle(&server, vec![node]).await;

        assert!(matches!(
            prs[0].review_waiting,
            ReviewWaiting::PushUnknown(_)
        ));
        assert!(format!("{:#}", failures[0].1).contains("head repository is gone"));
    }
}
