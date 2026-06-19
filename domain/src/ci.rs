use chrono::Duration;
use serde::{Deserialize, Serialize};

use crate::pr::RepoSlug;
use crate::urgency::Urgency;

/// A failed GitHub Actions run surfaced as a hub signal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CiFailure {
    /// GitHub repository the workflow belongs to.
    pub repo: RepoSlug,
    /// Name of the workflow (e.g. "CI").
    pub workflow_name: String,
    /// Name of the failing job, if known.
    pub job_name: Option<String>,
    /// Name of the failing step within the job, if known.
    pub step_name: Option<String>,
    /// Human-readable error message extracted from the run logs, if available.
    pub error: Option<String>,
    /// How long ago the failure was first observed.
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
    /// Computed urgency for ranking this failure against other signals.
    pub urgency: Urgency,
    /// Link to the GitHub Actions run page.
    pub url: String,
}
