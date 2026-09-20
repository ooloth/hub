//! Whether a refresh came back with anything worth caching.

use anyhow::{Context, Result};
use workflows::status::StatusReport;

/// What one refresh came back with.
#[derive(Debug)]
pub(crate) enum RefreshOutcome {
    /// At least one source answered. This is the queue as of now. An empty
    /// queue with no failures lands here and must be written, or nothing ever
    /// learns that the queue drained.
    Refreshed(FreshStatus),
    /// Every source failed. Nothing came back, so the cache keeps the row it
    /// already has rather than losing what was last known.
    NothingRefreshed {
        /// The sources that failed, named for the error message.
        failed_sources: Vec<String>,
    },
}

/// A status report at least one source answered for, so it is safe to replace
/// the cache with.
///
/// The field is private and [`classify`] is the only constructor, so a refresh
/// that came back with nothing cannot reach [`crate::cache::write`].
#[derive(Debug)]
pub(crate) struct FreshStatus(StatusReport);

impl FreshStatus {
    /// How many signals the refresh came back with.
    pub(crate) const fn item_count(&self) -> usize {
        self.0.items.len()
    }

    /// How many sources failed during the refresh.
    pub(crate) const fn failed_source_count(&self) -> usize {
        self.0.errors.len()
    }

    /// The JSON payload to store, in the shape the TUI reads.
    ///
    /// # Errors
    /// Returns an error if the report cannot be serialized.
    pub(crate) fn payload(&self) -> Result<String> {
        serde_json::to_string(&self.0).context("failed to serialize the status report")
    }
}

/// Decides whether a refresh came back with anything worth caching.
///
/// An empty report with no failures is a real answer: the queue drained, and
/// the cache has to say so. An empty report with failures is not an answer at
/// all, and writing it would discard what was last known.
pub(crate) fn classify(report: StatusReport) -> RefreshOutcome {
    if report.items.is_empty() && !report.errors.is_empty() {
        RefreshOutcome::NothingRefreshed {
            failed_sources: report.errors,
        }
    } else {
        RefreshOutcome::Refreshed(FreshStatus(report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workflows::status::StatusItem;

    fn ci_failure() -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("owner", "repo"),
            workflow_name: "CI".to_string(),
            job_name: None,
            step_name: None,
            error: None,
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/owner/repo/actions/runs/1".to_string(),
        })
    }

    fn report(items: usize, failed_sources: usize) -> StatusReport {
        StatusReport {
            items: (0..items).map(|_| ci_failure()).collect(),
            errors: (0..failed_sources).map(|i| format!("source {i}")).collect(),
        }
    }

    #[rstest::rstest]
    #[case::every_source_answered(1, 0)]
    #[case::some_sources_failed_but_others_answered(1, 1)]
    #[case::an_empty_queue_with_no_failures(0, 0)]
    fn a_refresh_that_came_back_with_an_answer_is_cached(
        #[case] items: usize,
        #[case] failed_sources: usize,
    ) {
        let outcome = classify(report(items, failed_sources));

        assert!(matches!(outcome, RefreshOutcome::Refreshed(_)));
    }

    #[test]
    fn a_refresh_where_every_source_failed_is_not_cached() {
        let outcome = classify(report(0, 2));

        match outcome {
            RefreshOutcome::NothingRefreshed { failed_sources } => {
                assert_eq!(failed_sources, vec!["source 0", "source 1"]);
            }
            RefreshOutcome::Refreshed(_) => {
                panic!("a refresh that reached no source must not be cached")
            }
        }
    }

    #[test]
    fn a_partial_refresh_reports_what_answered_and_what_failed() {
        let outcome = classify(report(2, 1));

        match outcome {
            RefreshOutcome::Refreshed(fresh) => {
                assert_eq!(fresh.item_count(), 2);
                assert_eq!(fresh.failed_source_count(), 1);
            }
            RefreshOutcome::NothingRefreshed { .. } => {
                panic!("a refresh that reached a source must be cached")
            }
        }
    }
}
