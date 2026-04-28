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
    pub age_days: u64,
}

pub struct Issue {
    pub number: u64,
    pub title: String,
    pub repo: RepoSlug,
    pub url: String,
    pub age_days: u64,
    pub labels: Vec<String>,
}
