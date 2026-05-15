use serde::{Deserialize, Serialize};

pub mod duration_secs {
    use chrono::Duration;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(d: &Duration, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        d.num_seconds().serialize(s)
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = i64::deserialize(d)?;
        Ok(Duration::seconds(secs))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Urgency {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepoSlug(String);

impl RepoSlug {
    pub fn new(owner: &str, repo: &str) -> Self {
        assert!(!owner.is_empty(), "owner must not be empty");
        assert!(!repo.is_empty(), "repo must not be empty");
        Self(format!("{owner}/{repo}"))
    }
}

impl std::fmt::Display for RepoSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PrKind {
    Mine,
    MyDraft,
    ToReview,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub repo: RepoSlug,
    pub url: String,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub kind: PrKind,
    pub author: String,
    pub review_decision: Option<ReviewDecision>,
    pub review_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub repo: RepoSlug,
    pub url: String,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub labels: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinearIssue {
    pub identifier: String,
    pub title: String,
    pub url: String,
    pub state: String,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CiFailure {
    pub repo: RepoSlug,
    pub workflow_name: String,
    pub job_name: Option<String>,
    pub step_name: Option<String>,
    pub error: Option<String>,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub url: String,
}

/// A Loki query to run for one monitoring scenario (e.g. app errors, worker panics).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiQuery {
    pub title: String,
    pub query: String,
    pub lookback: String,
    pub threshold: u32,
}

/// All Loki queries configured for one deployment environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiEnv {
    pub project: String,
    pub env: String,
    pub endpoint: String,
    pub token: Option<String>,
    pub grafana_url: Option<String>,
    pub queries: Vec<LokiQuery>,
}

/// One log entry returned by a Loki query that breached its threshold.
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
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_slug_new_formats_owner_and_repo() {
        let slug = RepoSlug::new("ooloth", "hub");
        assert_eq!(slug.to_string(), "ooloth/hub");
    }

    #[test]
    #[should_panic(expected = "owner must not be empty")]
    fn repo_slug_new_panics_on_empty_owner() {
        RepoSlug::new("", "hub");
    }

    #[test]
    #[should_panic(expected = "repo must not be empty")]
    fn repo_slug_new_panics_on_empty_repo() {
        RepoSlug::new("ooloth", "");
    }
}
