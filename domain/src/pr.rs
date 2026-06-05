use chrono::Duration;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepoSlug(String);

impl RepoSlug {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PrKind {
    Mine,
    MyDraft,
    ToReview,
    External,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CiStatus {
    Success,
    Failure,
    Pending,
    Neutral,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
    pub patch: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewComment {
    pub author: String,
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
    pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewThread {
    pub path: String,
    pub line: Option<u32>,
    pub comments: Vec<ReviewComment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub repo: RepoSlug,
    pub url: String,
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
    pub urgency: crate::urgency::Urgency,
    pub kind: PrKind,
    pub author: String,
    pub review_decision: Option<ReviewDecision>,
    #[serde(default)]
    pub approval_count: u32,
    #[serde(default)]
    pub comment_count: u32,
    pub head_branch: String,
    pub base_branch: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub ci_status: Option<CiStatus>,
    #[serde(default)]
    pub changed_files: Vec<ChangedFile>,
    #[serde(default)]
    pub total_changed_files: u32,
    #[serde(default)]
    pub review_threads: Vec<ReviewThread>,
    #[serde(default)]
    pub pr_comments: Vec<ReviewComment>,
    #[serde(default)]
    pub merge_blocker: Option<MergeBlocker>,
}
