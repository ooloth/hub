use chrono::Duration;
use secrecy::Secret;
use serde::{Deserialize, Serialize};

use crate::pr::RepoSlug;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Urgency {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CiFailure {
    pub repo: RepoSlug,
    pub workflow_name: String,
    pub job_name: Option<String>,
    pub step_name: Option<String>,
    pub error: Option<String>,
    #[serde(with = "crate::duration_secs")]
    pub age: Duration,
    pub urgency: Urgency,
    pub url: String,
}

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
    #[serde(with = "crate::duration_secs")]
    pub age: Duration,
    pub urgency: Urgency,
    pub url: String,
}

/// A GCP Cloud Logging query to run for one monitoring scenario.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GcpQuery {
    pub title: String,
    /// Raw GCP Logging filter string, fully user-controlled.
    pub query: String,
    pub lookback: String,
    /// JSON payload key used to extract a human-readable message from each entry.
    /// Falls back to text_payload first line, then the raw log line. Defaults to `"message"`.
    pub message_field: String,
}

/// All GCP Cloud Logging queries configured for one deployment environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GcpEnv {
    pub project: String,
    pub env: String,
    /// GCP project ID, used as the `resourceNames` scope for API calls.
    pub gcp_project: String,
    /// GCP region — used for GCP Console log links. Optional.
    pub gcp_region: Option<String>,
    pub queries: Vec<GcpQuery>,
}

/// One log entry returned by a GCP Cloud Logging query.
/// Emitted once per raw log line; the display layer groups by `message`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GcpEntry {
    pub title: String,
    pub project: String,
    pub env: String,
    /// Display + grouping key extracted from the log payload.
    pub message: String,
    /// Raw JSON log line, passed to investigation agents for context.
    pub line: String,
    pub lookback: String,
    #[serde(with = "crate::duration_secs")]
    pub age: Duration,
    pub urgency: Urgency,
    pub url: String,
    /// GCP cloud project ID (e.g. "rp006-prod-49a893d8"), distinct from the hub project name.
    #[serde(default)]
    pub gcp_project: String,
}
