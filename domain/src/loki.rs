use chrono::Duration;
use secrecy::Secret;
use serde::{Deserialize, Serialize};

use crate::urgency::Urgency;

/// A Loki query to run for one monitoring scenario (e.g. app errors, worker panics).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiQuery {
    pub title: String,
    pub query: String,
    pub lookback: String,
    /// Stream label key used to extract a human-readable message from each entry.
    /// Falls back to the raw log line when absent. Defaults to `"message"`.
    pub message_field: String,
}

/// All Loki queries configured for one deployment environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiEnv {
    pub project: String,
    pub env: String,
    pub endpoint: String,
    #[serde(skip)]
    pub token: Option<Secret<String>>,
    pub grafana_url: Option<String>,
    pub queries: Vec<LokiQuery>,
}

/// One log entry returned by a Loki query.
/// Emitted once per raw log line; the display layer groups by `message`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiEntry {
    pub title: String,
    pub project: String,
    pub env: String,
    /// The `message` stream label — stable error category, used as the grouping key.
    pub message: String,
    /// Raw JSON log line, passed to investigation agents for context.
    pub line: String,
    pub lookback: String,
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
    pub urgency: Urgency,
    pub url: String,
}
