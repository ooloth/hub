use anyhow::{Context, Result};
use domain::{
    ChangedFile, CiStatus, GithubPrsRepo, MergeBlocker, MergeReadiness, PrKind, PullRequest,
    RepoSlug, ReviewComment, ReviewDecision, ReviewThread, Urgency,
};
use serde::Deserialize;

use super::super::age;
use super::activity::{activity, ActivityNodes, ACTIVITY_FIELDS};

const GITHUB_API: &str = "https://api.github.com";

// ── GraphQL response types ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GraphQlResponse {
    /// `None` when GitHub refused the query, such as one over its node limit.
    data: Option<GraphQlData>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
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
    /// `None` when the author's account was deleted.
    author: Option<PrAuthor>,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    #[serde(rename = "reviewDecision")]
    review_decision: Option<String>,
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
    comments: PrCommentConnection,
    #[serde(default)]
    #[serde(rename = "mergeStateStatus")]
    merge_state_status: Option<String>,
    #[serde(flatten)]
    activity: ActivityNodes,
}

#[derive(Deserialize)]
struct PrAuthor {
    login: String,
}

/// The login GitHub shows for an account that was deleted.
const DELETED_ACCOUNT: &str = "ghost";

/// `author`'s login, or the one GitHub shows when the account was deleted.
fn login_or_ghost(author: Option<PrAuthor>) -> String {
    author.map_or_else(|| DELETED_ACCOUNT.to_string(), |a| a.login)
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
struct PrCommentConnection {
    nodes: Vec<PrCommentNode>,
}

#[derive(Deserialize)]
struct PrCommentNode {
    /// `None` when the commenter's account was deleted.
    author: Option<PrAuthor>,
    #[serde(rename = "createdAt")]
    created_at: String,
    body: String,
}

// ── Public fetch functions ────────────────────────────────────────────────────

/// Returns PRs across the given repos where review has been requested from `github_username`.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable, returns a non-2xx response, or refuses the
/// query.
pub async fn prs_awaiting_review(
    token: &str,
    repos: &[GithubPrsRepo],
    github_username: &str,
) -> Result<Vec<PullRequest>> {
    fetch_prs(
        GITHUB_API,
        token,
        &Search {
            query: "is:open is:pr review-requested:@me",
            urgency: Urgency::High,
            kind: PrKind::ToReview,
            only_assigned_to_me: false,
        },
        repos,
        github_username,
    )
    .await
}

/// Returns open non-draft PRs across the given repos not authored by `github_username` and not
/// where their review was explicitly requested.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable, returns a non-2xx response, or refuses the
/// query.
pub async fn external_prs(
    token: &str,
    repos: &[GithubPrsRepo],
    github_username: &str,
) -> Result<Vec<PullRequest>> {
    fetch_prs(
        GITHUB_API,
        token,
        &Search {
            query: "is:open is:pr -is:draft -author:@me -review-requested:@me",
            urgency: Urgency::Medium,
            kind: PrKind::External,
            only_assigned_to_me: false,
        },
        repos,
        github_username,
    )
    .await
}

/// Returns open non-draft PRs across the given repos authored by `github_username`, excluding any
/// PR assigned to someone other than `github_username`.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable, returns a non-2xx response, or refuses the
/// query.
pub async fn my_open_prs(
    token: &str,
    repos: &[GithubPrsRepo],
    github_username: &str,
) -> Result<Vec<PullRequest>> {
    fetch_prs(
        GITHUB_API,
        token,
        &Search {
            query: "is:open is:pr -is:draft author:@me",
            urgency: Urgency::High,
            kind: PrKind::Mine,
            only_assigned_to_me: true,
        },
        repos,
        github_username,
    )
    .await
}

/// Returns open draft PRs across the given repos authored by `github_username`, excluding any PR
/// assigned to someone other than `github_username`.
///
/// # Errors
/// Returns an error if the GitHub API is unreachable, returns a non-2xx response, or refuses the
/// query.
pub async fn my_draft_prs(
    token: &str,
    repos: &[GithubPrsRepo],
    github_username: &str,
) -> Result<Vec<PullRequest>> {
    fetch_prs(
        GITHUB_API,
        token,
        &Search {
            query: "is:open is:pr is:draft author:@me",
            urgency: Urgency::Medium,
            kind: PrKind::Mine,
            only_assigned_to_me: true,
        },
        repos,
        github_username,
    )
    .await
}

/// One search for pull requests, and what the pull requests it finds are to me.
struct Search {
    query: &'static str,
    urgency: Urgency,
    kind: PrKind,
    /// Whether a pull request assigned to someone else is left out.
    only_assigned_to_me: bool,
}

/// The pull requests `search` finds in `repos`, as `me` sees them, with their changed files'
/// patches. `base` is GitHub's API root.
async fn fetch_prs(
    base: &str,
    token: &str,
    search: &Search,
    repos: &[GithubPrsRepo],
    me: &str,
) -> Result<Vec<PullRequest>> {
    if repos.is_empty() {
        return Ok(vec![]);
    }
    let nodes = graphql_prs(base, token, search.query, repos).await?;
    let mut prs = nodes_to_prs(nodes, search, me, repos)?;
    enrich_with_file_patches(base, token, &mut prs).await;
    Ok(prs)
}

// ── Conversion ────────────────────────────────────────────────────────────────

fn nodes_to_prs(
    nodes: Vec<PrNode>,
    search: &Search,
    me: &str,
    repos: &[GithubPrsRepo],
) -> Result<Vec<PullRequest>> {
    nodes
        .into_iter()
        .filter(|node| {
            !search.only_assigned_to_me
                || node.assignees.nodes.iter().all(|a| a.login == me)
                || node.assignees.nodes.is_empty()
        })
        .filter(|node| {
            let excluded = repos
                .iter()
                .find(|r| r.repo == node.repository.name_with_owner)
                .is_some_and(|r| {
                    r.exclude_authors.iter().any(|a| {
                        node.author
                            .as_ref()
                            .is_some_and(|author| a == &author.login)
                    })
                });

            !excluded
        })
        .map(|node| node_to_pr(node, search, me))
        .collect()
}

fn node_to_pr(node: PrNode, search: &Search, me: &str) -> Result<PullRequest> {
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

    let activity = activity(&node.activity, me);
    let threads = node.activity.review_threads.nodes;

    let thread_comment_count: usize = threads.iter().map(|t| t.comments.nodes.len()).sum();
    let total_comments = thread_comment_count + node.comments.nodes.len();
    let comment_count = u32::try_from(total_comments).context("comment count overflow")?;

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
        review_threads: threads
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
                        author: c
                            .author
                            .map_or_else(|| DELETED_ACCOUNT.to_string(), |a| a.login),
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
                author: login_or_ghost(c.author),
                age: age(&c.created_at),
                body: c.body,
            })
            .collect(),
        age: age(&node.created_at),
        urgency: search.urgency,
        kind: search.kind,
        draft: node.is_draft,
        author: login_or_ghost(node.author),
        review_decision: parse_review_decision(node.review_decision.as_deref()),
        comment_count,
        head_branch: node.head_ref_name,
        base_branch: node.base_ref_name,
        merge_readiness: parse_merge_readiness(node.merge_state_status.as_deref()),
        activity,
    })
}

async fn fetch_file_patches(
    base: &str,
    token: &str,
    repo: &str,
    number: u64,
) -> std::collections::HashMap<String, Option<String>> {
    let result: Result<Vec<PrFileEntry>> = async {
        let entries: Vec<PrFileEntry> = reqwest::Client::new()
            .get(format!("{base}/repos/{repo}/pulls/{number}/files"))
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

async fn enrich_with_file_patches(base: &str, token: &str, prs: &mut [PullRequest]) {
    let futures: Vec<_> = prs
        .iter()
        .map(|pr| {
            let token = token.to_string();
            let repo = pr.repo.to_string();
            let number = pr.number;
            async move { fetch_file_patches(base, &token, &repo, number).await }
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
        "REVIEW_REQUIRED" => Some(ReviewDecision::ReviewRequired),
        _ => None,
    }
}

// ── GraphQL query ─────────────────────────────────────────────────────────────

/// The pull requests GitHub's search for `base_query` finds in `repos`, each with every field a
/// pull request is read from. `base` is GitHub's API root.
async fn graphql_prs(
    base: &str,
    token: &str,
    base_query: &str,
    repos: &[GithubPrsRepo],
) -> Result<Vec<PrNode>> {
    let repo_filters = repos
        .iter()
        .map(|r| format!("repo:{}", r.repo))
        .collect::<Vec<_>>()
        .join(" ");

    let q = format!("{base_query} {repo_filters}");

    let query = format!(
        r#"{{ search(query: "{q}", type: ISSUE, first: 100) {{ nodes {{ ... on PullRequest {{
            number title url body
            author {{ login }}
            createdAt isDraft reviewDecision mergeStateStatus
            headRefName baseRefName
            repository {{ nameWithOwner }}
            assignees(first: 10) {{ nodes {{ login }} }}
            commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state }} }} }} }}
            files(first: 100) {{ totalCount nodes {{ path additions deletions }} }}
            comments(first: 50) {{ nodes {{ author {{ login }} createdAt body }} }}
            {ACTIVITY_FIELDS}
        }} }} }} }}"#
    );

    let response: GraphQlResponse = reqwest::Client::new()
        .post(format!("{base}/graphql"))
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

    search_nodes(response)
}

/// The pull requests in `response`, or why GitHub refused the query.
///
/// GitHub can answer with data and errors together, when it could not resolve a field for one
/// pull request. The data is still the answer then, so only a response without data fails.
fn search_nodes(response: GraphQlResponse) -> Result<Vec<PrNode>> {
    let Some(data) = response.data else {
        let messages: Vec<String> = response.errors.into_iter().map(|e| e.message).collect();
        anyhow::bail!(
            "GitHub refused the pull request query: {}",
            messages.join("; ")
        );
    };
    Ok(data.search.nodes)
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

    /// A search result shaped like GitHub's, with no activity on it.
    fn node_json(author: &str, repo: &str, assignees: &[&str], draft: bool) -> serde_json::Value {
        serde_json::json!({
            "number": 1,
            "title": "title",
            "url": "https://github.com/owner/repo/pull/1",
            "body": null,
            "author": { "login": author },
            "createdAt": "2024-01-01T00:00:00Z",
            "isDraft": draft,
            "reviewDecision": null,
            "mergeStateStatus": "CLEAN",
            "headRefName": "feat/test",
            "baseRefName": "main",
            "repository": { "nameWithOwner": repo },
            "assignees": { "nodes": assignees.iter().map(|l| serde_json::json!({ "login": l })).collect::<Vec<_>>() },
            "commits": { "nodes": [] },
            "files": { "totalCount": 0, "nodes": [] },
            "comments": { "nodes": [] },
            "latestOpinionatedReviews": { "nodes": [] },
            "reviewRequests": { "nodes": [] },
            "reviewThreads": { "nodes": [] },
            "recentReviews": { "nodes": [] },
            "recentComments": { "nodes": [] }
        })
    }

    fn node(author: &str, repo: &str, assignees: &[&str]) -> PrNode {
        serde_json::from_value(node_json(author, repo, assignees, false)).unwrap()
    }

    fn search(kind: PrKind, only_assigned_to_me: bool) -> Search {
        Search {
            query: "is:open is:pr",
            urgency: Urgency::Medium,
            kind,
            only_assigned_to_me,
        }
    }

    fn pr_repo(repo: &str, exclude_authors: Vec<&str>) -> GithubPrsRepo {
        GithubPrsRepo {
            repo: repo.into(),
            exclude_authors: exclude_authors.into_iter().map(String::from).collect(),
        }
    }

    #[rstest::rstest]
    #[case::unassigned(&[], 1)]
    #[case::assigned_to_me(&["alice"], 1)]
    #[case::assigned_to_someone_else(&["bob"], 0)]
    fn my_pull_requests_assigned_to_someone_else_are_left_out(
        #[case] assignees: &[&str],
        #[case] kept: usize,
    ) {
        let result = nodes_to_prs(
            vec![node("alice", "owner/repo", assignees)],
            &search(PrKind::Mine, true),
            "alice",
            &[],
        )
        .unwrap();
        assert_eq!(result.len(), kept);
    }

    #[test]
    fn nodes_to_prs_excludes_pr_from_repo_excluded_author() {
        let result = nodes_to_prs(
            vec![node("dependabot-preview[bot]", "owner/a", &[])],
            &search(PrKind::ToReview, false),
            "me",
            &[pr_repo("owner/a", vec!["dependabot-preview[bot]"])],
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn nodes_to_prs_keeps_pr_when_author_excluded_only_in_different_repo() {
        let result = nodes_to_prs(
            vec![node("dependabot", "owner/b", &[])],
            &search(PrKind::ToReview, false),
            "me",
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
        let result = nodes_to_prs(
            vec![node("alice", "owner/a", &[])],
            &search(PrKind::ToReview, false),
            "me",
            &[pr_repo("owner/a", vec!["dependabot"])],
        )
        .unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn nodes_to_prs_with_empty_exclude_lists_passes_all_prs() {
        let result = nodes_to_prs(
            vec![node("alice", "owner/a", &[]), node("bob", "owner/a", &[])],
            &search(PrKind::ToReview, false),
            "me",
            &[pr_repo("owner/a", vec![])],
        )
        .unwrap();
        assert_eq!(result.len(), 2);
    }

    // ── drafts ────────────────────────────────────────────────────────────────

    /// A draft someone else opened and asked me to review stays theirs (#380).
    #[test]
    fn a_draft_keeps_the_kind_its_search_gave_it() {
        let draft: PrNode =
            serde_json::from_value(node_json("ada", "owner/repo", &[], true)).unwrap();

        let pr = node_to_pr(draft, &search(PrKind::ToReview, false), "me").unwrap();

        assert_eq!(pr.kind, PrKind::ToReview);
        assert!(pr.draft);
    }

    // ── parse_review_decision ─────────────────────────────────────────────────

    #[rstest::rstest]
    #[case(Some("APPROVED"), Some(ReviewDecision::Approved))]
    #[case(Some("CHANGES_REQUESTED"), Some(ReviewDecision::ChangesRequested))]
    #[case(Some("REVIEW_REQUIRED"), Some(ReviewDecision::ReviewRequired))]
    #[case(Some("SOMETHING_NEW"), None)]
    #[case(None, None)]
    fn a_review_decision_is_read_as_github_gave_it(
        #[case] input: Option<&str>,
        #[case] expected: Option<ReviewDecision>,
    ) {
        assert_eq!(parse_review_decision(input), expected);
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
        #[case] state: Option<&str>,
        #[case] expected: MergeReadiness,
    ) {
        assert_eq!(parse_merge_readiness(state), expected);
    }

    // ── fetching ──────────────────────────────────────────────────────────────

    async fn github(graphql_body: serde_json::Value) -> wiremock::MockServer {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/graphql"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(graphql_body))
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path_regex(
                r"^/repos/.+/pulls/\d+/files$",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;
        server
    }

    async fn fetch_mine(server: &wiremock::MockServer) -> Result<Vec<PullRequest>> {
        fetch_prs(
            &server.uri(),
            "token",
            &search(PrKind::Mine, true),
            &[pr_repo("owner/repo", vec![])],
            "alice",
        )
        .await
    }

    #[tokio::test]
    async fn a_refused_query_fails_with_githubs_reason() {
        let server = github(serde_json::json!({
            "data": null,
            "errors": [{ "type": "MAX_NODE_LIMIT_EXCEEDED", "message": "This query requests up to 598,200 possible nodes which exceeds the maximum limit of 500,000." }]
        }))
        .await;

        let error = fetch_mine(&server).await.unwrap_err();

        assert!(
            format!("{error:#}").contains("exceeds the maximum limit of 500,000"),
            "{error:#}"
        );
    }

    /// Whose move it is comes from the search alone: one query, and a request per pull request for
    /// its changed files' patches, with no lookup of anything else.
    #[tokio::test]
    async fn a_refresh_reads_whose_move_it_is_from_the_search_alone() {
        let server = github(serde_json::json!({ "data": { "search": { "nodes": [
            node_json("alice", "owner/repo", &[], false)
        ] } } }))
        .await;

        let prs = fetch_mine(&server).await.unwrap();

        let paths: Vec<String> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| request.url.path().to_string())
            .collect();
        assert_eq!(prs.len(), 1);
        assert_eq!(paths.len(), 2, "{paths:?}");
        assert!(paths.contains(&"/graphql".to_string()), "{paths:?}");
        assert!(
            paths.contains(&"/repos/owner/repo/pulls/1/files".to_string()),
            "{paths:?}"
        );
    }

    /// A field GitHub could not resolve for one pull request comes back as an error beside the
    /// data, and the rest of the search is still usable.
    #[tokio::test]
    async fn an_error_beside_data_still_returns_the_search() {
        let server = github(serde_json::json!({
            "data": { "search": { "nodes": [node_json("alice", "owner/repo", &[], false)] } },
            "errors": [{ "type": "FORBIDDEN", "message": "Resource protected by organization SAML enforcement." }]
        }))
        .await;

        let prs = fetch_mine(&server).await.unwrap();

        assert_eq!(prs.len(), 1);
    }

    /// GitHub returns `null` for the author of a deleted account.
    #[test]
    fn a_pull_request_and_comment_from_deleted_accounts_still_parse() {
        let mut json = node_json("alice", "owner/repo", &[], false);
        json["author"] = serde_json::Value::Null;
        json["comments"] = serde_json::json!({ "nodes": [
            { "author": null, "createdAt": "2024-01-01T00:00:00Z", "body": "gone" }
        ] });
        let node: PrNode = serde_json::from_value(json).unwrap();

        let pr = node_to_pr(node, &search(PrKind::External, false), "me").unwrap();

        assert_eq!(pr.author, "ghost");
        assert_eq!(pr.pr_comments[0].author, "ghost");
    }
}
