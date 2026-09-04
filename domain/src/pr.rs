use chrono::Duration;
use serde::{Deserialize, Serialize};

/// A validated `owner/repo` identifier for a GitHub repository.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepoSlug(String);

impl RepoSlug {
    /// # Panics
    /// Panics if `owner` or `repo` is empty.
    #[must_use]
    pub fn new(owner: &str, repo: &str) -> Self {
        assert!(!owner.is_empty(), "owner must not be empty");
        assert!(!repo.is_empty(), "repo must not be empty");
        Self(format!("{owner}/{repo}"))
    }
}

impl std::fmt::Display for RepoSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// How a pull request relates to the current user — used for filtering and urgency.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PrKind {
    /// Authored by the current user and ready for review.
    Mine,
    /// Authored by the current user but still a draft.
    MyDraft,
    /// Opened by someone else and awaiting the current user's review.
    ToReview,
    /// Not directly involving the current user.
    External,
}

/// The aggregate review decision on a pull request from GitHub's GraphQL API.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReviewDecision {
    /// At least one reviewer approved and no pending change requests remain.
    Approved,
    /// One or more reviewers requested changes.
    ChangesRequested,
}

/// The review state of a pull request, resolved from `approval_count`,
/// `changes_requested_count` and `review_decision` into one consistent value.
///
/// Counts are trusted first since `review_decision` can be `null` even when
/// reviews exist (e.g. no branch-protection policy requires one).
/// `review_decision` is used only as a fallback when both counts are zero,
/// so a real GitHub decision is never silently dropped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewStatus {
    /// No approvals and no changes-requested reviews.
    NoReviews,
    /// `Some(n)` is a counted approval total (n > 0). `None` means a decision
    /// of `Approved` was trusted from `review_decision` with no counted
    /// approvals to report (the fallback case) — no count is invented.
    Approved(Option<u32>),
    /// At least one changes-requested review exists (or is trusted from
    /// `review_decision` as a fallback). Carries no count: the word alone
    /// is shown, regardless of how many approvals also exist.
    ChangesRequested,
}

impl PullRequest {
    /// Resolves this PR's review state, preferring counted reviews over the
    /// aggregate `review_decision`, which can be `null` even when reviews exist.
    #[must_use]
    pub const fn review_status(&self) -> ReviewStatus {
        if self.changes_requested_count > 0 {
            return ReviewStatus::ChangesRequested;
        }
        if self.approval_count > 0 {
            return ReviewStatus::Approved(Some(self.approval_count));
        }
        match self.review_decision {
            Some(ReviewDecision::ChangesRequested) => ReviewStatus::ChangesRequested,
            Some(ReviewDecision::Approved) => ReviewStatus::Approved(None),
            None => ReviewStatus::NoReviews,
        }
    }
}

/// The rolled-up CI status for a pull request's head commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CiStatus {
    /// All required checks passed.
    Success,
    /// At least one required check failed.
    Failure,
    /// Checks are still running.
    Pending,
    /// No required checks are configured or all checks were skipped.
    Neutral,
}

/// The resolution state of a pull request, as returned by GitHub's GraphQL API.
///
/// Used by the fold-back step to decide whether a PR-origin task should
/// transition to `done` or `failed` when its linked PR leaves the open state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PrState {
    /// Pull request is still open.
    Open,
    /// Merged into the base branch.
    Merged,
    /// Closed without merging.
    Closed,
}

/// Why a pull request cannot be merged right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MergeBlocker {
    /// Branch has a merge conflict (mergeStateStatus: DIRTY).
    Conflict,
    /// Branch is behind the base branch (mergeStateStatus: BEHIND).
    Behind,
    /// Merge is blocked by a branch protection rule (mergeStateStatus: BLOCKED).
    Blocked,
}

impl std::str::FromStr for RepoSlug {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('/') {
            Some((owner, repo)) if !owner.is_empty() && !repo.is_empty() && !repo.contains('/') => {
                Ok(Self(s.to_string()))
            }
            _ => Err(format!(
                "invalid repo slug {s:?}: expected \"owner/repo\" with non-empty parts and no extra slashes"
            )),
        }
    }
}

impl RepoSlug {
    /// Returns the repository name portion of the slug (everything after the `/`).
    #[must_use]
    pub fn repo_name(&self) -> &str {
        self.0.split_once('/').map_or(&self.0, |(_, repo)| repo)
    }
}

/// A file changed by a pull request, with diff statistics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangedFile {
    /// File path relative to the repository root.
    pub path: String,
    /// Number of lines added.
    pub additions: u32,
    /// Number of lines deleted.
    pub deletions: u32,
    /// Unified diff patch for this file, if fetched.
    pub patch: Option<String>,
}

/// A single review comment left on a pull request or review thread.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewComment {
    /// GitHub username of the comment author.
    pub author: String,
    /// How long ago the comment was posted.
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
    /// Comment body text.
    pub body: String,
}

/// A review thread anchored to a specific location in a pull request diff.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewThread {
    /// File path the thread is anchored to.
    pub path: String,
    /// Line number within the file, if the thread is on a specific line.
    pub line: Option<u32>,
    /// Comments in this thread, oldest first.
    pub comments: Vec<ReviewComment>,
}

/// A GitHub pull request surfaced as a hub signal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PullRequest {
    /// Pull request number within the repository.
    pub number: u64,
    /// Pull request title.
    pub title: String,
    /// Repository the pull request belongs to.
    pub repo: RepoSlug,
    /// Link to the pull request on GitHub.
    pub url: String,
    /// How long ago the pull request was opened.
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
    /// Computed urgency for ranking this PR against other signals.
    pub urgency: crate::urgency::Urgency,
    /// How this PR relates to the current user.
    pub kind: PrKind,
    /// GitHub username of the PR author.
    pub author: String,
    /// Aggregate review decision, if any reviewers have responded.
    pub review_decision: Option<ReviewDecision>,
    /// Number of approvals the PR has received.
    #[serde(default)]
    pub approval_count: u32,
    /// Number of changes-requested reviews the PR has received.
    #[serde(default)]
    pub changes_requested_count: u32,
    /// Number of review comments on the PR.
    #[serde(default)]
    pub comment_count: u32,
    /// Name of the source branch being merged.
    pub head_branch: String,
    /// Name of the target branch the PR merges into.
    pub base_branch: String,
    /// PR body text, if fetched.
    #[serde(default)]
    pub body: Option<String>,
    /// Rolled-up CI status for the head commit, if available.
    #[serde(default)]
    pub ci_status: Option<CiStatus>,
    /// Subset of changed files fetched for context (may be partial).
    #[serde(default)]
    pub changed_files: Vec<ChangedFile>,
    /// Total number of files changed, including any not in `changed_files`.
    #[serde(default)]
    pub total_changed_files: u32,
    /// Inline review threads on the PR diff.
    #[serde(default)]
    pub review_threads: Vec<ReviewThread>,
    /// Top-level (non-diff) comments on the PR.
    #[serde(default)]
    pub pr_comments: Vec<ReviewComment>,
    /// Reason the PR cannot be merged right now, if any.
    #[serde(default)]
    pub merge_blocker: Option<MergeBlocker>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case("ooloth/hub", "hub")]
    #[case("org/my-app", "my-app")]
    #[case("a/b", "b")]
    fn repo_name_returns_last_segment(#[case] slug: &str, #[case] expected: &str) {
        let s: RepoSlug = slug.parse().unwrap();
        assert_eq!(s.repo_name(), expected);
    }

    #[test]
    fn from_str_roundtrips_valid_slug() {
        let s: RepoSlug = "ooloth/hub".parse().unwrap();
        assert_eq!(s.to_string(), "ooloth/hub");
    }

    #[rstest::rstest]
    #[case("hub", "no slash")]
    #[case("/hub", "empty owner")]
    #[case("ooloth/", "empty repo")]
    #[case("", "empty string")]
    #[case("a/b/c", "extra slash")]
    fn from_str_rejects_invalid_slugs(#[case] input: &str, #[case] _reason: &str) {
        assert!(
            input.parse::<RepoSlug>().is_err(),
            "expected error for {input:?}"
        );
    }

    // ── review_status ─────────────────────────────────────────────────────────

    fn pr_with_review_state(
        review_decision: Option<ReviewDecision>,
        approval_count: u32,
        changes_requested_count: u32,
    ) -> PullRequest {
        PullRequest {
            number: 1,
            title: "title".to_string(),
            repo: RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/1".to_string(),
            age: Duration::zero(),
            urgency: crate::urgency::Urgency::Medium,
            kind: PrKind::ToReview,
            author: "alice".to_string(),
            review_decision,
            approval_count,
            changes_requested_count,
            comment_count: 0,
            head_branch: "feat/thing".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: None,
        }
    }

    #[test]
    fn review_status_no_reviews_when_no_decision_and_no_counts() {
        let pr = pr_with_review_state(None, 0, 0);
        assert_eq!(pr.review_status(), ReviewStatus::NoReviews);
    }

    #[test]
    fn review_status_reports_approval_count_even_when_decision_is_null() {
        // Regression: GitHub's reviewDecision can be null (e.g. no required-review
        // branch protection) even when a raw APPROVED review exists.
        let pr = pr_with_review_state(None, 1, 0);
        assert_eq!(pr.review_status(), ReviewStatus::Approved(Some(1)));
    }

    #[test]
    fn review_status_reports_changes_requested_even_when_decision_is_null() {
        let pr = pr_with_review_state(None, 0, 1);
        assert_eq!(pr.review_status(), ReviewStatus::ChangesRequested);
    }

    #[test]
    fn review_status_changes_requested_trumps_approvals() {
        let pr = pr_with_review_state(None, 2, 1);
        assert_eq!(pr.review_status(), ReviewStatus::ChangesRequested);
    }

    #[test]
    fn review_status_trusts_approved_decision_when_counts_are_zero() {
        // Fallback for the (rare) case where reviewDecision reports a real
        // decision but the counted reviews found nothing — never fabricates a count.
        let pr = pr_with_review_state(Some(ReviewDecision::Approved), 0, 0);
        assert_eq!(pr.review_status(), ReviewStatus::Approved(None));
    }

    #[test]
    fn review_status_trusts_changes_requested_decision_when_counts_are_zero() {
        let pr = pr_with_review_state(Some(ReviewDecision::ChangesRequested), 0, 0);
        assert_eq!(pr.review_status(), ReviewStatus::ChangesRequested);
    }

    #[test]
    fn review_status_uses_approval_count_when_decision_agrees() {
        let pr = pr_with_review_state(Some(ReviewDecision::Approved), 3, 0);
        assert_eq!(pr.review_status(), ReviewStatus::Approved(Some(3)));
    }
}
