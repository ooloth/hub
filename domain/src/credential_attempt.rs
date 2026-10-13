//! One failed attempt by the daemon to obtain its credentials, as it is logged and recorded.
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::credential_failure::CredentialFailure;

/// One attempt to resolve every credential in `hub.toml` that failed. Not a pass: no source was
/// asked and the cached payload was not touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialAttempt {
    /// When the attempt started.
    pub started_at: DateTime<Utc>,
    /// How long it ran before it failed.
    pub duration: Duration,
    /// The daemon process that made it. The same one on every attempt means nothing restarted.
    pub pid: u32,
    /// Which attempt this was since the daemon started, counting from 1.
    pub attempt: u32,
    /// Why it failed.
    pub failure: CredentialFailure,
    /// How long until the next attempt, or `None` when there will be none.
    pub retry_in: Option<Duration>,
}
