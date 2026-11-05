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
    /// GitHub login of the issue author, as GraphQL spells it: an app's login has no `[bot]`, so
    /// Renovate is `renovate`, the same spelling `PullRequest.author` holds.
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
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    /// Either relabelling: marking an issue ready for an agent, or dismissing it.
    type Relabel = fn(&[String]) -> Vec<String>;

    fn labels(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[rstest]
    #[case::ready_replaces_needs_review(
        agent_ready_labels,
        &[NEEDS_HUMAN_REVIEW_LABEL],
        &[READY_FOR_AGENT_LABEL]
    )]
    #[case::ready_drops_needs_review_beside_ready(
        agent_ready_labels,
        &[NEEDS_HUMAN_REVIEW_LABEL, READY_FOR_AGENT_LABEL],
        &[READY_FOR_AGENT_LABEL]
    )]
    #[case::ready_leaves_a_ready_issue_unchanged(
        agent_ready_labels,
        &[READY_FOR_AGENT_LABEL],
        &[READY_FOR_AGENT_LABEL]
    )]
    #[case::ready_keeps_other_labels_in_order(
        agent_ready_labels,
        &["bug", "wontfix"],
        &["bug", "wontfix", READY_FOR_AGENT_LABEL]
    )]
    #[case::ready_labels_an_unlabelled_issue(agent_ready_labels, &[], &[READY_FOR_AGENT_LABEL])]
    #[case::ready_keeps_others_and_drops_needs_review(
        agent_ready_labels,
        &["bug", NEEDS_HUMAN_REVIEW_LABEL],
        &["bug", READY_FOR_AGENT_LABEL]
    )]
    #[case::dismissed_replaces_needs_review(
        dismissed_labels,
        &[NEEDS_HUMAN_REVIEW_LABEL],
        &[WONTFIX_LABEL]
    )]
    #[case::dismissed_drops_needs_review_beside_wontfix(
        dismissed_labels,
        &[NEEDS_HUMAN_REVIEW_LABEL, WONTFIX_LABEL],
        &[WONTFIX_LABEL]
    )]
    #[case::dismissed_leaves_a_dismissed_issue_unchanged(
        dismissed_labels,
        &[WONTFIX_LABEL],
        &[WONTFIX_LABEL]
    )]
    #[case::dismissed_keeps_other_labels_in_order(
        dismissed_labels,
        &["bug"],
        &["bug", WONTFIX_LABEL]
    )]
    #[case::dismissed_labels_an_unlabelled_issue(dismissed_labels, &[], &[WONTFIX_LABEL])]
    #[case::dismissed_keeps_others_and_drops_needs_review(
        dismissed_labels,
        &["bug", NEEDS_HUMAN_REVIEW_LABEL],
        &["bug", WONTFIX_LABEL]
    )]
    fn relabelling_swaps_needs_review_for_its_label_and_keeps_the_rest(
        #[case] relabel: Relabel,
        #[case] before: &[&str],
        #[case] after: &[&str],
    ) {
        assert_eq!(relabel(&labels(before)), labels(after));
    }

    /// Label sets dense with the labels relabelling looks for, among arbitrary others.
    fn label_sets() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec(
            prop_oneof![
                Just(NEEDS_HUMAN_REVIEW_LABEL.to_string()),
                Just(READY_FOR_AGENT_LABEL.to_string()),
                Just(WONTFIX_LABEL.to_string()),
                "[a-z:-]{1,24}",
            ],
            0..6,
        )
    }

    #[rstest]
    #[case::ready_for_agent(agent_ready_labels, READY_FOR_AGENT_LABEL)]
    #[case::dismissed(dismissed_labels, WONTFIX_LABEL)]
    fn relabelling_any_issue_drops_needs_review_adds_its_label_and_is_idempotent(
        #[case] relabel: Relabel,
        #[case] label: &str,
    ) {
        proptest!(|(before in label_sets())| {
            let once = relabel(&before);

            prop_assert!(!once.iter().any(|l| l == NEEDS_HUMAN_REVIEW_LABEL), "{once:?}");
            prop_assert!(once.iter().any(|l| l == label), "{once:?}");
            prop_assert_eq!(relabel(&once), once);
        });
    }
}
