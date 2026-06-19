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
