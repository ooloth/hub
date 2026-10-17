//! The line a daemon leaves in the log when it starts.
use std::fmt;

use chrono::{DateTime, Utc};
use domain::profile::Profile;

use crate::pass::one_line;

/// Printed once the daemon holds its profile's lock and before it loads credentials, which can
/// wait on a 1Password prompt for up to 90 seconds an attempt. A startup line with nothing after it
/// is a daemon in its first attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Startup {
    pub(crate) at: DateTime<Utc>,
    pub(crate) pid: u32,
    pub(crate) profile: Profile,
}

impl fmt::Display for Startup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "hub-daemon start at={} pid={} profile={}",
            self.at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            self.pid,
            self.profile.as_str()
        )
    }
}

/// Printed instead of anyhow's `Error: …` when the daemon fails before its first pass, so the log
/// says when each failed start happened. launchd restarts the daemon at most once a minute, and
/// these lines are how that shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupFailure {
    pub(crate) at: DateTime<Utc>,
    pub(crate) pid: u32,
    /// `None` when `HUB_PROFILE` itself was what failed.
    pub(crate) profile: Option<Profile>,
    /// The error chain, as `{:#}` prints it.
    pub(crate) error: String,
}

impl fmt::Display for StartupFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "hub-daemon start_failed at={} pid={}",
            self.at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            self.pid
        )?;
        if let Some(profile) = self.profile {
            write!(f, " profile={}", profile.as_str())?;
        }
        write!(f, " error=\"{}\"", one_line(&self.error))
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn the_startup_line_names_the_time_pid_and_profile_the_way_pass_lines_do() {
        let startup = Startup {
            at: Utc.with_ymd_and_hms(2026, 10, 3, 23, 15, 55).unwrap(),
            pid: 4242,
            profile: Profile::Default,
        };

        assert_eq!(
            startup.to_string(),
            "hub-daemon start at=2026-10-03T23:15:55Z pid=4242 profile=default"
        );
    }

    fn failure(profile: Option<Profile>, error: &str) -> StartupFailure {
        StartupFailure {
            at: Utc.with_ymd_and_hms(2026, 10, 10, 5, 14, 29).unwrap(),
            pid: 4242,
            profile,
            error: error.to_string(),
        }
    }

    #[test]
    fn a_startup_failure_names_the_time_pid_profile_and_error() {
        let line = failure(
            Some(Profile::Default),
            "hub-daemon is already running for profile default (pid 86297)",
        )
        .to_string();

        assert_eq!(
            line,
            "hub-daemon start_failed at=2026-10-10T05:14:29Z pid=4242 profile=default \
             error=\"hub-daemon is already running for profile default (pid 86297)\""
        );
    }

    #[test]
    fn a_failure_before_the_profile_is_known_omits_it() {
        let line = failure(None, "HUB_PROFILE must be default or dev").to_string();

        assert!(!line.contains("profile="), "{line}");
    }

    #[test]
    fn a_multi_line_error_with_quotes_stays_on_one_line_inside_its_quotes() {
        let line = failure(
            Some(Profile::Default),
            "failed to load hub config: could not read secret\n  \"op://Scripts/x\"",
        )
        .to_string();

        assert_eq!(line.lines().count(), 1);
        assert!(
            line.ends_with(
                "error=\"failed to load hub config: could not read secret 'op://Scripts/x'\""
            ),
            "{line}"
        );
    }
}
