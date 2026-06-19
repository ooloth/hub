use chrono::Duration;
use serde::{Deserialize, Serialize};

use crate::urgency::Urgency;

/// A GCP Cloud Logging query to run for one monitoring scenario.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GcpQuery {
    /// Human-readable label shown in the TUI for this query.
    pub title: String,
    /// Raw GCP Logging filter string, fully user-controlled.
    pub query: String,
    /// How far back to query (e.g. "15m", "1h").
    pub lookback: String,
    /// JSON payload key used to extract a human-readable message from each entry.
    /// Falls back to `text_payload` first line, then the raw log line. Defaults to `"message"`.
    pub message_field: String,
}

/// All GCP Cloud Logging queries configured for one deployment environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GcpEnv {
    /// Hub project name (e.g. "myapp"), used as the grouping key in signals.
    pub project: String,
    /// Deployment environment label (e.g. "prod", "staging").
    pub env: String,
    /// GCP project ID, used as the `resourceNames` scope for API calls.
    pub gcp_project: String,
    /// GCP region — used for GCP Console log links. Optional.
    pub gcp_region: Option<String>,
    /// Queries to run against this environment.
    pub queries: Vec<GcpQuery>,
}

/// One log entry returned by a GCP Cloud Logging query.
/// Emitted once per raw log line; the display layer groups by `message`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GcpEntry {
    /// Human-readable label for the query that produced this entry.
    pub title: String,
    /// Hub project name this entry belongs to.
    pub project: String,
    /// Deployment environment this entry came from (e.g. "prod").
    pub env: String,
    /// Display + grouping key extracted from the log payload.
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
    /// Link to the GCP Console log viewer for this entry.
    pub url: String,
    /// GCP cloud project ID (e.g. "rp006-prod-49a893d8"), distinct from the hub project name.
    #[serde(default)]
    pub gcp_project: String,
}
