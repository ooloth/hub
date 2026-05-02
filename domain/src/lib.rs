#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Urgency {
    Critical,
    High,
    Medium,
    Low,
}

pub struct RepoSlug(String);

impl RepoSlug {
    pub fn new(owner: &str, repo: &str) -> Self {
        Self(format!("{owner}/{repo}"))
    }
}

impl std::fmt::Display for RepoSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub repo: RepoSlug,
    pub url: String,
    pub age: chrono::Duration,
    pub urgency: Urgency,
}

pub struct Issue {
    pub number: u64,
    pub title: String,
    pub repo: RepoSlug,
    pub url: String,
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub labels: Vec<String>,
}

pub struct LinearIssue {
    pub identifier: String,
    pub title: String,
    pub url: String,
    pub state: String,
    pub age: chrono::Duration,
    pub urgency: Urgency,
}

pub struct CiFailure {
    pub repo: RepoSlug,
    pub workflow_name: String,
    pub conclusion: String,
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub url: String,
}
