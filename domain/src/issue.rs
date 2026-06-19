use chrono::Duration;
use serde::{Deserialize, Serialize};

use crate::pr::RepoSlug;
use crate::urgency::Urgency;

pub const NEEDS_HUMAN_REVIEW_LABEL: &str = "status:needs-human-review";
pub const READY_FOR_AGENT_LABEL: &str = "status:ready-for-agent";
pub const WONTFIX_LABEL: &str = "wontfix";

/// Returns the label set that marks an issue as ready for an agent:
/// removes `NEEDS_HUMAN_REVIEW_LABEL` (if present) and adds `READY_FOR_AGENT_LABEL`
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

/// Returns the label set that dismisses an issue as won't fix:
/// removes `NEEDS_HUMAN_REVIEW_LABEL` (if present) and adds `WONTFIX_LABEL`
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub repo: RepoSlug,
    pub url: String,
    pub author: String,
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
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
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
    pub urgency: Urgency,
}

/// The resolved state of an issue, normalised across platforms.
///
/// Raw API values (GitHub `stateReason`, Linear state `type`) are mapped to
/// this enum in the client layer before reaching domain or workflow logic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueState {
    Open,
    Completed,
    Cancelled,
}

/// A GitHub repository configured for the github-prs workflow, with the PR authors to exclude.
#[derive(Clone, Debug)]
pub struct GithubPrsRepo {
    pub repo: String,
    pub exclude_authors: Vec<String>,
}
