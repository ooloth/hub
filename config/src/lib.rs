pub mod toml;

use anyhow::{Context, Result};

pub struct Config {
    pub github_token: String,
    pub linear_token: Option<String>,
    pub projects: Vec<toml::Project>,
    pub monitor: Option<toml::Monitor>,
}

impl Config {
    /// Loads config from `hub.toml` and environment variables.
    ///
    /// # Errors
    /// Returns an error if `hub.toml` is missing or malformed, or if `GITHUB_TOKEN` is not set.
    pub fn load() -> Result<Self> {
        let hub_toml = toml::parse_file("hub.toml")?;
        Ok(Self {
            github_token: std::env::var("GITHUB_TOKEN").context("GITHUB_TOKEN not set")?,
            linear_token: std::env::var("LINEAR_TOKEN").ok(),
            projects: hub_toml.project,
            monitor: hub_toml.monitor,
        })
    }

    pub fn github_pr_repos(&self) -> Vec<String> {
        self.projects
            .iter()
            .filter(|p| {
                p.workflow
                    .iter()
                    .any(|w| matches!(w, toml::WorkflowConfig::GithubPrs { .. }))
            })
            .map(|p| p.repo.clone())
            .collect()
    }

    pub fn github_open_issue_repos(&self) -> Vec<String> {
        self.projects
            .iter()
            .filter_map(|p| {
                p.workflow.iter().find_map(|w| {
                    if let toml::WorkflowConfig::GithubIssues {
                        assigned_only: false,
                        ..
                    } = w
                    {
                        Some(p.repo.clone())
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    pub fn github_assigned_issue_repos(&self) -> Vec<String> {
        self.projects
            .iter()
            .filter_map(|p| {
                p.workflow.iter().find_map(|w| {
                    if let toml::WorkflowConfig::GithubIssues {
                        assigned_only: true,
                        ..
                    } = w
                    {
                        Some(p.repo.clone())
                    } else {
                        None
                    }
                })
            })
            .collect()
    }
}
