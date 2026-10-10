//! The line a daemon leaves in the log when it starts.
use std::fmt;

use chrono::{DateTime, Utc};
use domain::profile::Profile;

/// Printed once the daemon holds its profile's lock and before it loads credentials, which can
/// wait on 1Password prompts with no end. A startup line with no pass line after it is a daemon
/// waiting on credentials.
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
}
