use anyhow::{Context, Result};
use domain::{
    ChangedFile, CiStatus, GithubPrsRepo, MergeBlocker, PrKind, PullRequest, RepoSlug,
    ReviewComment, ReviewDecision, ReviewThread, Urgency,
};
use serde::Deserialize;

use super::super::age;

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

// ── GraphQL query ─────────────────────────────────────────────────────────────

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
}
