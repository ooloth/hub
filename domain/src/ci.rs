use chrono::Duration;
use serde::{Deserialize, Serialize};

use crate::pr::RepoSlug;
use crate::urgency::Urgency;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CiFailure {
    pub repo: RepoSlug,
    pub workflow_name: String,
    pub job_name: Option<String>,
    pub step_name: Option<String>,
    pub error: Option<String>,
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
    pub urgency: Urgency,
    pub url: String,
}
