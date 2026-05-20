use serde::{Deserialize, Serialize};

pub mod duration_secs {
    use chrono::Duration;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(d: &Duration, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        d.num_seconds().serialize(s)
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = i64::deserialize(d)?;
        Ok(Duration::seconds(secs))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Urgency {
    Critical,
    High,
    Medium,
    Low,
}

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
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
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
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub kind: PrKind,
    pub author: String,
    pub review_decision: Option<ReviewDecision>,
    pub review_count: u32,
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
}

pub const NEEDS_HUMAN_REVIEW_LABEL: &str = "status:needs-human-review";
pub const READY_FOR_AGENT_LABEL: &str = "status:ready-for-agent";
pub const WONTFIX_LABEL: &str = "wontfix";

/// Returns the label set that marks an issue as ready for an agent:
/// removes `NEEDS_HUMAN_REVIEW_LABEL` (if present) and adds `READY_FOR_AGENT_LABEL`
/// (if not already present). All other labels are preserved in order. Idempotent.
pub fn agent_ready_labels(labels: &[String]) -> Vec<String> {
    let mut result: Vec<String> = labels
        .iter()
        .filter(|l| l.as_str() != NEEDS_HUMAN_REVIEW_LABEL)
        .cloned()
        .collect();
    if !result.iter().any(|l| l.as_str() == READY_FOR_AGENT_LABEL) {
        result.push(READY_FOR_AGENT_LABEL.to_string());
    }
    result
}

/// Returns the label set that dismisses an issue as won't fix:
/// removes `NEEDS_HUMAN_REVIEW_LABEL` (if present) and adds `WONTFIX_LABEL`
/// (if not already present). All other labels are preserved in order. Idempotent.
pub fn dismissed_labels(labels: &[String]) -> Vec<String> {
    let mut result: Vec<String> = labels
        .iter()
        .filter(|l| l.as_str() != NEEDS_HUMAN_REVIEW_LABEL)
        .cloned()
        .collect();
    if !result.iter().any(|l| l.as_str() == WONTFIX_LABEL) {
        result.push(WONTFIX_LABEL.to_string());
    }
    result
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub repo: RepoSlug,
    pub url: String,
    pub author: String,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub labels: Vec<String>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinearIssue {
    pub identifier: String,
    pub title: String,
    pub url: String,
    pub state: String,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CiFailure {
    pub repo: RepoSlug,
    pub workflow_name: String,
    pub job_name: Option<String>,
    pub step_name: Option<String>,
    pub error: Option<String>,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub url: String,
}

/// A Loki query to run for one monitoring scenario (e.g. app errors, worker panics).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiQuery {
    pub title: String,
    pub query: String,
    pub lookback: String,
    pub threshold: u32,
}

/// A GitHub repository configured for the github-prs workflow, with the PR authors to exclude.
#[derive(Clone, Debug)]
pub struct GithubPrsRepo {
    pub repo: String,
    pub exclude_authors: Vec<String>,
}

/// All Loki queries configured for one deployment environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiEnv {
    pub project: String,
    pub env: String,
    pub endpoint: String,
    pub token: Option<String>,
    pub grafana_url: Option<String>,
    pub queries: Vec<LokiQuery>,
}

/// One log entry returned by a Loki query that breached its threshold.
/// Emitted once per raw log line; the display layer groups by `message`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiEntry {
    pub title: String,
    pub project: String,
    pub env: String,
    /// The `message` stream label — stable error category, used as the grouping key.
    pub message: String,
    /// Raw JSON log line, passed to investigation agents for context.
    pub line: String,
    pub lookback: String,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── agent_ready_labels ────────────────────────────────────────────────────

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[rstest::rstest]
    // needs-human-review removed, ready-for-agent added
    #[case(vec![s(NEEDS_HUMAN_REVIEW_LABEL)], vec![s(READY_FOR_AGENT_LABEL)])]
    // already has ready-for-agent and needs-human-review → only needs-human-review removed
    #[case(
        vec![s(NEEDS_HUMAN_REVIEW_LABEL), s(READY_FOR_AGENT_LABEL)],
        vec![s(READY_FOR_AGENT_LABEL)]
    )]
    // already fully ready → unchanged (idempotent)
    #[case(vec![s(READY_FOR_AGENT_LABEL)], vec![s(READY_FOR_AGENT_LABEL)])]
    // no relevant labels → ready-for-agent added, others preserved
    #[case(vec![s("bug"), s("wontfix")], vec![s("bug"), s("wontfix"), s(READY_FOR_AGENT_LABEL)])]
    // empty → only ready-for-agent
    #[case(vec![], vec![s(READY_FOR_AGENT_LABEL)])]
    // unrelated label + needs-human-review → unrelated preserved, needs-human-review removed
    #[case(
        vec![s("bug"), s(NEEDS_HUMAN_REVIEW_LABEL)],
        vec![s("bug"), s(READY_FOR_AGENT_LABEL)]
    )]
    fn agent_ready_labels_cases(#[case] input: Vec<String>, #[case] expected: Vec<String>) {
        assert_eq!(agent_ready_labels(&input), expected);
    }

    #[test]
    fn agent_ready_labels_idempotent() {
        let input = vec![s(NEEDS_HUMAN_REVIEW_LABEL), s("bug")];
        let once = agent_ready_labels(&input);
        let twice = agent_ready_labels(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn agent_ready_labels_never_contains_needs_human_review() {
        // property: result never contains NEEDS_HUMAN_REVIEW_LABEL, regardless of input
        let inputs: &[&[&str]] = &[
            &[NEEDS_HUMAN_REVIEW_LABEL],
            &[NEEDS_HUMAN_REVIEW_LABEL, READY_FOR_AGENT_LABEL],
            &["bug", NEEDS_HUMAN_REVIEW_LABEL, "wontfix"],
            &[],
        ];
        for labels in inputs {
            let input: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
            let result = agent_ready_labels(&input);
            assert!(
                !result.iter().any(|l| l == NEEDS_HUMAN_REVIEW_LABEL),
                "result contained NEEDS_HUMAN_REVIEW_LABEL for input {labels:?}"
            );
            assert!(
                result.iter().any(|l| l == READY_FOR_AGENT_LABEL),
                "result missing READY_FOR_AGENT_LABEL for input {labels:?}"
            );
        }
    }

    // ── dismissed_labels ──────────────────────────────────────────────────────

    #[rstest::rstest]
    // needs-human-review removed, wontfix added
    #[case(vec![s(NEEDS_HUMAN_REVIEW_LABEL)], vec![s(WONTFIX_LABEL)])]
    // already has wontfix and needs-human-review → only needs-human-review removed
    #[case(
        vec![s(NEEDS_HUMAN_REVIEW_LABEL), s(WONTFIX_LABEL)],
        vec![s(WONTFIX_LABEL)]
    )]
    // already dismissed → unchanged (idempotent)
    #[case(vec![s(WONTFIX_LABEL)], vec![s(WONTFIX_LABEL)])]
    // no relevant labels → wontfix added, others preserved
    #[case(vec![s("bug")], vec![s("bug"), s(WONTFIX_LABEL)])]
    // empty → only wontfix
    #[case(vec![], vec![s(WONTFIX_LABEL)])]
    // unrelated label + needs-human-review → unrelated preserved, needs-human-review removed
    #[case(
        vec![s("bug"), s(NEEDS_HUMAN_REVIEW_LABEL)],
        vec![s("bug"), s(WONTFIX_LABEL)]
    )]
    fn dismissed_labels_cases(#[case] input: Vec<String>, #[case] expected: Vec<String>) {
        assert_eq!(dismissed_labels(&input), expected);
    }

    #[test]
    fn dismissed_labels_idempotent() {
        let input = vec![s(NEEDS_HUMAN_REVIEW_LABEL), s("bug")];
        let once = dismissed_labels(&input);
        let twice = dismissed_labels(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn dismissed_labels_never_contains_needs_human_review() {
        let inputs: &[&[&str]] = &[
            &[NEEDS_HUMAN_REVIEW_LABEL],
            &[NEEDS_HUMAN_REVIEW_LABEL, WONTFIX_LABEL],
            &["bug", NEEDS_HUMAN_REVIEW_LABEL],
            &[],
        ];
        for labels in inputs {
            let input: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
            let result = dismissed_labels(&input);
            assert!(
                !result.iter().any(|l| l == NEEDS_HUMAN_REVIEW_LABEL),
                "result contained NEEDS_HUMAN_REVIEW_LABEL for input {labels:?}"
            );
            assert!(
                result.iter().any(|l| l == WONTFIX_LABEL),
                "result missing WONTFIX_LABEL for input {labels:?}"
            );
        }
    }

    // ── repo_slug ─────────────────────────────────────────────────────────────

    #[test]
    fn repo_slug_new_formats_owner_and_repo() {
        let slug = RepoSlug::new("ooloth", "hub");
        assert_eq!(slug.to_string(), "ooloth/hub");
    }

    #[test]
    #[should_panic(expected = "owner must not be empty")]
    fn repo_slug_new_panics_on_empty_owner() {
        RepoSlug::new("", "hub");
    }

    #[test]
    #[should_panic(expected = "repo must not be empty")]
    fn repo_slug_new_panics_on_empty_repo() {
        RepoSlug::new("ooloth", "");
    }
}
