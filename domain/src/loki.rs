use chrono::Duration;
use secrecy::Secret;
use serde::{Deserialize, Serialize};

use crate::urgency::Urgency;

/// A Loki query to run for one monitoring scenario (e.g. app errors, worker panics).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiQuery {
    /// Human-readable label shown in the TUI for this query.
    pub title: String,
    /// `LogQL` query string.
    pub query: String,
    /// How far back to query (e.g. "15m", "1h").
    pub lookback: String,
    /// Stream label key used to extract a human-readable message from each entry.
    /// Falls back to the raw log line when absent. Defaults to `"message"`.
    pub message_field: String,
}

/// All Loki queries configured for one deployment environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiEnv {
    /// Hub project name (e.g. "myapp"), used as the grouping key in signals.
    pub project: String,
    /// Deployment environment label (e.g. "prod", "staging").
    pub env: String,
    /// Loki push/query endpoint URL.
    pub endpoint: String,
    /// Bearer token for authenticating with the Loki endpoint.
    #[serde(skip)]
    pub token: Option<Secret<String>>,
    /// Grafana base URL used for building deep-link URLs. Optional.
    pub grafana_url: Option<String>,
    /// Queries to run against this environment.
    pub queries: Vec<LokiQuery>,
}

/// One log entry returned by a Loki query.
/// Emitted once per raw log line; the display layer groups by `message`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiEntry {
    /// Human-readable label for the query that produced this entry.
    pub title: String,
    /// Hub project name this entry belongs to.
    pub project: String,
    /// Deployment environment this entry came from (e.g. "prod").
    pub env: String,
    /// The `message` stream label — stable error category, used as the grouping key.
    pub message: String,
    /// Raw JSON log line, passed to investigation agents for context.
    pub line: String,
    /// Lookback window that produced this entry (e.g. "15m").
    pub lookback: String,
    /// How long ago this log entry was emitted.
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
    /// Computed urgency for ranking this entry against other signals.
    pub urgency: Urgency,
    /// Link to the Grafana explore view for this log entry.
    pub url: String,
}
