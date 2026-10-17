//! One failed attempt by the daemon to obtain its credentials, as it is logged and recorded.
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::credential_failure::CredentialFailure;
use crate::known_secrets::KnownSecrets;
use crate::source_failure::FailureReason;

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
    /// What happened when the daemon opened the 1Password app after this attempt found it not
    /// running, or `None` when it did not try.
    pub opened_app: Option<AppOpening>,
    /// How long until the next attempt, or `None` when there will be none.
    pub retry_in: Option<Duration>,
}

/// The daemon's attempt to open the 1Password app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppOpening {
    /// `open -a 1Password` succeeded.
    Opened,
    /// It failed.
    Failed {
        /// Why.
        reason: FailureReason,
    },
}

impl AppOpening {
    /// Opening failed with `error`, redacted of every value in `secrets`.
    ///
    /// # Panics
    /// If the redacted reason still holds a known secret, spans lines, or is too long. Only a bug in
    /// redaction can cause any of these.
    #[must_use]
    pub fn failed(error: &anyhow::Error, secrets: &KnownSecrets) -> Self {
        Self::Failed {
            reason: FailureReason::redacted(error, secrets, "opening the 1Password app"),
        }
    }
}
