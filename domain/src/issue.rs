use chrono::Duration;
use serde::{Deserialize, Serialize};

use crate::pr::RepoSlug;
use crate::urgency::Urgency;

/// GitHub label applied when an issue requires human review before an agent can proceed.
pub const NEEDS_HUMAN_REVIEW_LABEL: &str = "status:needs-human-review";
/// GitHub label applied when an issue is cleared for agent work.
pub const READY_FOR_AGENT_LABEL: &str = "status:ready-for-agent";
/// GitHub label applied when an issue is dismissed as won't fix.
pub const WONTFIX_LABEL: &str = "wontfix";

/// Returns the label set that marks an issue as ready for an agent.
///
/// Removes `NEEDS_HUMAN_REVIEW_LABEL` (if present) and adds `READY_FOR_AGENT_LABEL`
/// (if not already present). All other labels are preserved in order. Idempotent.
#[must_use]
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

/// Returns the label set that dismisses an issue as won't fix.
///
/// Removes `NEEDS_HUMAN_REVIEW_LABEL` (if present) and adds `WONTFIX_LABEL`
/// (if not already present). All other labels are preserved in order. Idempotent.
#[must_use]
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

/// A GitHub issue surfaced as a hub signal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    /// GitHub issue number.
    pub number: u64,
    /// Issue title.
    pub title: String,
    /// Repository the issue belongs to.
    pub repo: RepoSlug,
    /// Link to the issue on GitHub.
    pub url: String,
    /// GitHub username of the issue author.
    pub author: String,
    /// How long ago the issue was opened.
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
    /// Computed urgency for ranking this issue against other signals.
    pub urgency: Urgency,
    /// GitHub labels currently applied to the issue.
    pub labels: Vec<String>,
    /// Issue body text, if fetched.
    #[serde(default)]
    pub body: Option<String>,
}

/// A Linear issue surfaced as a hub signal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinearIssue {
    /// Globally unique Linear identifier (e.g. "ENG-123").
    pub identifier: String,
    /// Issue title.
    pub title: String,
    /// Link to the issue on Linear.
    pub url: String,
    /// Current Linear workflow state name (e.g. "In Progress").
    pub state: String,
    /// How long ago the issue was created.
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
    /// Computed urgency for ranking this issue against other signals.
    pub urgency: Urgency,
}

/// The resolved state of an issue, normalised across platforms.
///
/// Raw API values (GitHub `stateReason`, Linear state `type`) are mapped to
/// this enum in the client layer before reaching domain or workflow logic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueState {
    /// Issue is open and active.
    Open,
    /// Issue was resolved as completed.
    Completed,
    /// Issue was closed without completion.
    Cancelled,
}

/// A GitHub repository configured for the github-prs workflow, with the PR authors to exclude.
#[derive(Clone, Debug)]
pub struct GithubPrsRepo {
    /// Repository slug in `owner/repo` format.
    pub repo: String,
    /// GitHub usernames whose PRs should be excluded from the signal list.
    pub exclude_authors: Vec<String>,
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
}
