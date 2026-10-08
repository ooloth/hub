//! Whether a refresh came back with anything worth caching.

use anyhow::{Context, Result};
use domain::source_outcomes::SourceOutcomes;
use store::status_cache::Payload;
use workflows::status::Refresh;

/// What one refresh came back with.
#[derive(Debug)]
pub(crate) enum RefreshOutcome {
    /// At least one source answered with something, or nothing failed. This is the queue as of
    /// now. An empty queue with no failures lands here and must be written, or nothing ever
    /// learns that the queue drained.
    Refreshed(FreshStatus),
    /// Something failed and nothing that answered had any signals. The cache keeps the row it
    /// already has rather than losing what was last known.
    NothingRefreshed {
        /// Which sources answered, and each that failed with why.
        sources: SourceOutcomes,
    },
}

/// A status report safe to replace the cache with.
///
/// The field is private and [`classify`] is the only constructor, so a refresh
/// that came back with nothing cannot reach the cache.
#[derive(Debug)]
pub(crate) struct FreshStatus(Refresh);

impl FreshStatus {
    /// How many signals the refresh came back with.
    pub(crate) const fn item_count(&self) -> usize {
        self.0.report.items.len()
    }

    /// Which sources answered, and each that failed with why.
    pub(crate) fn sources(&self) -> SourceOutcomes {
        self.0.sources.clone()
    }

    /// The payload to store, in the shape the TUI reads.
    ///
    /// # Errors
    /// Returns an error if the report cannot be serialized.
    pub(crate) fn payload(&self) -> Result<FreshPayload> {
        let json = serde_json::to_string(&self.0.report)
            .context("failed to serialize the status report")?;
        Ok(FreshPayload(Payload {
            json,
            schema_version: workflows::status::SCHEMA_VERSION,
        }))
    }
}

/// A payload serialized from a [`FreshStatus`].
///
/// The field is private and [`FreshStatus::payload`] is the only constructor, so a refresh that
/// came back with nothing cannot reach [`crate::cache::record`] as a payload.
#[derive(Debug)]
pub(crate) struct FreshPayload(Payload);

impl FreshPayload {
    /// The payload as the store writes it.
    pub(crate) const fn payload(&self) -> &Payload {
        &self.0
    }
}

/// Decides whether a refresh came back with anything worth caching.
///
/// An empty report with no failures is a real answer: the queue drained, and
/// the cache has to say so. An empty report with failures is not trusted as
/// one, and writing it would discard what was last known.
pub(crate) fn classify(refresh: Refresh) -> RefreshOutcome {
    if refresh.report.items.is_empty() && !refresh.sources.failed().is_empty() {
        let sources = refresh.sources;
        assert!(
            !sources.failed().is_empty(),
            "a refresh that keeps the cache had a failure"
        );
        RefreshOutcome::NothingRefreshed { sources }
    } else {
        assert!(
            !refresh.report.items.is_empty() || refresh.sources.failed().is_empty(),
            "a refresh that empties the cache had no failure"
        );
        RefreshOutcome::Refreshed(FreshStatus(refresh))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::known_secrets::KnownSecrets;
    use domain::source_failure::SourceFailure;
    use domain::source_outcomes::Reach;
    use workflows::status::{StatusItem, StatusReport};

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

    fn report(items: usize, failed_sources: usize) -> Refresh {
        refresh(items, &["answered"], failed_sources)
    }

    fn refresh(items: usize, answered: &[&str], failed_sources: usize) -> Refresh {
        let failures: Vec<SourceFailure> = (0..failed_sources)
            .map(|i| {
                SourceFailure::new(
                    format!("source {i}"),
                    &anyhow::anyhow!("down"),
                    &KnownSecrets::new(Vec::new()),
                )
            })
            .collect();
        Refresh {
            report: StatusReport {
                items: (0..items).map(|_| ci_failure()).collect(),
                errors: failures.iter().map(|f| f.source().to_string()).collect(),
            },
            sources: SourceOutcomes::new(
                answered.iter().map(|name| (*name).to_string()).collect(),
                failures,
            ),
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
        let outcome = classify(refresh(0, &[], 2));

        match outcome {
            RefreshOutcome::NothingRefreshed { sources } => {
                let failed: Vec<&str> =
                    sources.failed().iter().map(SourceFailure::source).collect();
                assert_eq!(failed, vec!["source 0", "source 1"]);
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
                assert_eq!(fresh.sources().failed().len(), 1);
            }
            RefreshOutcome::NothingRefreshed { .. } => {
                panic!("a refresh that reached a source must be cached")
            }
        }
    }

    #[test]
    fn a_refresh_where_one_source_failed_and_the_rest_found_nothing_keeps_the_cache_but_says_some_answered(
    ) {
        let outcome = classify(refresh(0, &["github issues", "linear issues"], 1));

        match outcome {
            RefreshOutcome::NothingRefreshed { sources } => {
                assert_eq!(sources.reach(), Reach::SomeSources);
            }
            RefreshOutcome::Refreshed(_) => {
                panic!("an empty refresh with a failure must not be cached")
            }
        }
    }

    #[test]
    fn a_payload_reads_back_as_the_same_status_report() {
        let RefreshOutcome::Refreshed(fresh) = classify(report(3, 1)) else {
            panic!("a refresh that reached a source must be cached")
        };

        let payload = fresh.payload().unwrap();

        let round_tripped: StatusReport = serde_json::from_str(&payload.payload().json).unwrap();
        assert_eq!(round_tripped.items.len(), 3);
        assert_eq!(round_tripped.errors, vec!["source 0"]);
    }

    #[test]
    fn a_payload_is_stamped_with_the_schema_version_the_tui_reads() {
        let RefreshOutcome::Refreshed(fresh) = classify(report(1, 0)) else {
            panic!("a refresh that reached a source must be cached")
        };

        let payload = fresh.payload().unwrap();

        assert_eq!(
            payload.payload().schema_version,
            workflows::status::SCHEMA_VERSION
        );
    }
}
