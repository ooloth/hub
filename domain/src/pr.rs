use chrono::Duration;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    pub fn repo_name(&self) -> &str {
        self.0
            .split_once('/')
            .map(|(_, repo)| repo)
            .unwrap_or(&self.0)
    }
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
}
