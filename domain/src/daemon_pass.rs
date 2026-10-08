//! One refresh pass of the daemon: when it ran, what it did with the cached payload, and how far
//! it reached.
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::pass_failure::PassFailure;
use crate::source_outcomes::{Reach, SourceOutcomes};

/// One pass, as the daemon logs it and records it in its health record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pass {
    /// When the pass started.
    pub started_at: DateTime<Utc>,
    /// How long it ran before it was recorded.
    pub duration: Duration,
    /// The daemon process that ran it. A different one on every pass means the daemon restarts.
    pub pid: u32,
    /// What it did with the cached payload.
    pub outcome: PassOutcome,
}

impl Pass {
    /// This pass, recorded as having failed with `failure` instead of whatever it did.
    ///
    /// For when recording the pass itself fails: the start, duration and process stay as they
    /// were.
    #[must_use]
    pub fn failed_with(&self, failure: PassFailure) -> Self {
        Self {
            outcome: PassOutcome::Failed { failure },
            ..self.clone()
        }
    }
}

/// What a pass did with the cached payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassOutcome {
    /// The payload was replaced with `items` signals.
    Wrote {
        /// How many signals the new payload holds.
        items: usize,
        /// Which sources answered, and which failed.
        sources: SourceOutcomes,
    },
    /// The payload was kept: something failed, and nothing that answered had any signals.
    Kept {
        /// Which sources answered, and which failed.
        sources: SourceOutcomes,
    },
    /// The pass itself failed.
    Failed {
        /// Why.
        failure: PassFailure,
    },
}

/// The four outcomes a reader of the health record tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassStatus {
    /// Every source answered.
    Ok,
    /// Some sources failed while at least one answered.
    Partial,
    /// Every source failed.
    NoSourceAnswered,
    /// The pass itself failed.
    PassFailed,
}

impl PassStatus {
    /// The word the log line and the health record use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Partial => "partial",
            Self::NoSourceAnswered => "no_source_answered",
            Self::PassFailed => "pass_failed",
        }
    }
}

impl PassOutcome {
    /// Which of the four outcomes this is.
    #[must_use]
    pub fn status(&self) -> PassStatus {
        match self.sources().map(SourceOutcomes::reach) {
            Some(Reach::EverySource) => PassStatus::Ok,
            Some(Reach::SomeSources) => PassStatus::Partial,
            Some(Reach::NoSource) => PassStatus::NoSourceAnswered,
            None => PassStatus::PassFailed,
        }
    }

    /// Whether the payload was replaced.
    #[must_use]
    pub const fn payload_written(&self) -> bool {
        matches!(self, Self::Wrote { .. })
    }

    /// How many signals the new payload holds, when one was written.
    #[must_use]
    pub const fn items(&self) -> Option<usize> {
        match self {
            Self::Wrote { items, .. } => Some(*items),
            Self::Kept { .. } | Self::Failed { .. } => None,
        }
    }

    /// Which sources answered and which failed, when the refresh ran.
    #[must_use]
    pub const fn sources(&self) -> Option<&SourceOutcomes> {
        match self {
            Self::Wrote { sources, .. } | Self::Kept { sources } => Some(sources),
            Self::Failed { .. } => None,
        }
    }

    /// Why the pass failed, when it did.
    #[must_use]
    pub const fn failure(&self) -> Option<&PassFailure> {
        match self {
            Self::Failed { failure } => Some(failure),
            Self::Wrote { .. } | Self::Kept { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::known_secrets::KnownSecrets;
    use crate::source_failure::SourceFailure;

    fn no_secrets() -> KnownSecrets {
        KnownSecrets::new(Vec::new())
    }

    fn sources(answered: &[&str], failed: &[&str]) -> SourceOutcomes {
        SourceOutcomes::new(
            answered.iter().map(|name| (*name).to_string()).collect(),
            failed
                .iter()
                .map(|name| SourceFailure::new(*name, &anyhow::anyhow!("down"), &no_secrets()))
                .collect(),
        )
    }

    #[rstest]
    #[case::every_source_answered(
        PassOutcome::Wrote { items: 3, sources: sources(&["a", "b"], &[]) },
        PassStatus::Ok,
        true
    )]
    #[case::partial_with_the_payload_written(
        PassOutcome::Wrote { items: 3, sources: sources(&["a"], &["b"]) },
        PassStatus::Partial,
        true
    )]
    #[case::partial_with_the_payload_kept(
        PassOutcome::Kept { sources: sources(&["a"], &["b"]) },
        PassStatus::Partial,
        false
    )]
    #[case::no_source_answered(
        PassOutcome::Kept { sources: sources(&[], &["a", "b"]) },
        PassStatus::NoSourceAnswered,
        false
    )]
    #[case::pass_failed(
        PassOutcome::Failed { failure: PassFailure::new(&anyhow::anyhow!("locked"), &no_secrets()) },
        PassStatus::PassFailed,
        false
    )]
    fn each_outcome_reads_as_its_status_and_says_whether_the_payload_was_written(
        #[case] outcome: PassOutcome,
        #[case] status: PassStatus,
        #[case] payload_written: bool,
    ) {
        assert_eq!(outcome.status(), status);
        assert_eq!(outcome.payload_written(), payload_written);
    }

    #[test]
    fn a_pass_that_failed_to_be_recorded_keeps_when_and_where_it_ran() {
        let pass = Pass {
            started_at: DateTime::from_timestamp(1_760_000_000, 0).unwrap(),
            duration: Duration::from_millis(6412),
            pid: 4242,
            outcome: PassOutcome::Kept {
                sources: sources(&[], &["a"]),
            },
        };
        let failure = PassFailure::new(&anyhow::anyhow!("disk full"), &no_secrets());

        let failed = pass.failed_with(failure.clone());

        assert_eq!(failed.started_at, pass.started_at);
        assert_eq!(failed.duration, pass.duration);
        assert_eq!(failed.pid, pass.pid);
        assert_eq!(failed.outcome, PassOutcome::Failed { failure });
    }
}
