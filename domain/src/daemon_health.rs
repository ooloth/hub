//! What the daemon's health record says about it, and what that means for whoever is looking.
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::daemon_pass::PassStatus;

/// How long past its interval a pass may start before the daemon counts as overdue. A pass is
/// recorded only once it finishes, so this also covers how long one takes.
pub const GRACE: Duration = Duration::from_mins(2);

/// How the daemon that wrote a record runs its passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    /// One pass, with `--once`, and nothing after it.
    Once,
    /// A pass every this long, for as long as the daemon runs. Never zero.
    Every(Duration),
}

impl Schedule {
    /// A pass every `interval`.
    ///
    /// # Panics
    /// If `interval` is zero. `--interval` refuses zero when it is parsed, so only a bug can pass
    /// one here.
    #[must_use]
    pub fn every(interval: Duration) -> Self {
        assert!(
            !interval.is_zero(),
            "the interval between passes is never zero"
        );
        Self::Every(interval)
    }
}

/// What the daemon was doing when it wrote its last record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastActivity {
    /// A pass, which ended this way.
    Pass(PassStatus),
    /// An attempt to obtain credentials, which failed, so no pass could run.
    WaitingForCredentials,
}

/// The daemon's last record, as this build reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaemonHealth {
    /// When the pass or the attempt started.
    pub started_at: DateTime<Utc>,
    /// How the daemon that wrote it runs.
    pub schedule: Schedule,
    /// Whether it was a pass or an attempt to obtain credentials.
    pub activity: LastActivity,
}

/// What the profile's database holds about the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DaemonRecord {
    /// No daemon has written a record for this profile.
    #[default]
    NoneRecorded,
    /// A daemon from another hub version wrote one, in a shape this build does not read.
    OtherVersion,
    /// The record, as written.
    Recorded(DaemonHealth),
}

/// What the daemon's record means for the signals and notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonStatus {
    /// It is refreshing on schedule.
    Current,
    /// Its last pass started longer ago than its interval allows.
    Overdue {
        /// How long ago it started.
        age: Duration,
    },
    /// It is still trying to obtain credentials, so it cannot refresh.
    WaitingForCredentials,
    /// It ran one pass with `--once`, so nothing refreshes after it.
    RanOnce,
    /// No daemon has run for this profile.
    NoneRecorded,
    /// A daemon from another hub version wrote the record, so this build cannot tell.
    OtherVersion,
}

impl DaemonRecord {
    /// What this record means at `now`.
    #[must_use]
    pub fn status(&self, now: DateTime<Utc>) -> DaemonStatus {
        let health = match self {
            Self::NoneRecorded => return DaemonStatus::NoneRecorded,
            Self::OtherVersion => return DaemonStatus::OtherVersion,
            Self::Recorded(health) => health,
        };
        let interval = match health.schedule {
            Schedule::Once => return DaemonStatus::RanOnce,
            Schedule::Every(interval) => interval,
        };
        // A start after now means the wall clock was set back since, which says nothing about
        // whether the daemon is late.
        let age = (now - health.started_at).to_std().unwrap_or_default();
        if age > interval + GRACE {
            return DaemonStatus::Overdue { age };
        }
        match health.activity {
            LastActivity::Pass(_) => DaemonStatus::Current,
            LastActivity::WaitingForCredentials => DaemonStatus::WaitingForCredentials,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_760_000_000 + seconds, 0).unwrap()
    }

    fn recorded(schedule: Schedule, activity: LastActivity) -> DaemonRecord {
        DaemonRecord::Recorded(DaemonHealth {
            started_at: at(0),
            schedule,
            activity,
        })
    }

    const FIFTEEN_MINUTES: Duration = Duration::from_secs(15 * 60);

    proptest! {
        #[test]
        fn a_pass_is_overdue_exactly_when_older_than_its_interval_plus_the_grace(
            interval_secs in 1u64..=3 * 60 * 60,
            age_secs in 0u64..=7 * 60 * 60,
        ) {
            let record = recorded(
                Schedule::every(Duration::from_secs(interval_secs)),
                LastActivity::Pass(PassStatus::Ok),
            );
            let limit = interval_secs + GRACE.as_secs();

            let status = record.status(at(i64::try_from(age_secs).unwrap()));

            if age_secs > limit {
                prop_assert_eq!(status, DaemonStatus::Overdue { age: Duration::from_secs(age_secs) });
            } else {
                prop_assert_eq!(status, DaemonStatus::Current);
            }
        }
    }

    #[rstest]
    #[case::within_the_limit(17 * 60, DaemonStatus::WaitingForCredentials)]
    #[case::past_the_limit(17 * 60 + 1, DaemonStatus::Overdue { age: Duration::from_secs(17 * 60 + 1) })]
    fn waiting_for_credentials_turns_overdue_once_past_the_limit(
        #[case] age_secs: i64,
        #[case] expected: DaemonStatus,
    ) {
        let record = recorded(
            Schedule::every(FIFTEEN_MINUTES),
            LastActivity::WaitingForCredentials,
        );

        assert_eq!(record.status(at(age_secs)), expected);
    }

    #[rstest]
    #[case::just_now(0)]
    #[case::long_ago(30 * 24 * 60 * 60)]
    fn a_once_pass_ran_once_whatever_its_age(#[case] age_secs: i64) {
        let record = recorded(Schedule::Once, LastActivity::Pass(PassStatus::Ok));

        assert_eq!(record.status(at(age_secs)), DaemonStatus::RanOnce);
    }

    #[test]
    fn a_pass_that_started_after_now_is_current() {
        let record = recorded(
            Schedule::every(FIFTEEN_MINUTES),
            LastActivity::Pass(PassStatus::Ok),
        );

        assert_eq!(record.status(at(-60 * 60)), DaemonStatus::Current);
    }

    #[rstest]
    #[case::none_recorded(DaemonRecord::NoneRecorded, DaemonStatus::NoneRecorded)]
    #[case::other_version(DaemonRecord::OtherVersion, DaemonStatus::OtherVersion)]
    fn a_record_this_build_has_nothing_from_says_so(
        #[case] record: DaemonRecord,
        #[case] expected: DaemonStatus,
    ) {
        assert_eq!(record.status(at(0)), expected);
    }

    #[test]
    #[should_panic(expected = "the interval between passes is never zero")]
    fn a_zero_interval_is_a_bug() {
        let _ = Schedule::every(Duration::ZERO);
    }
}
