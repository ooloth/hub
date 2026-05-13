pub mod toml;

use anyhow::{Context, Result};

pub struct Config {
    pub github_token: String,
    pub github_username: String,
    pub linear_token: Option<String>,
    pub loki_token: Option<String>,
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
            github_username: std::env::var("GITHUB_USERNAME").context("GITHUB_USERNAME not set")?,
            linear_token: std::env::var("LINEAR_TOKEN").ok(),
            loki_token: std::env::var("LOKI_TOKEN").ok(),
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

    /// Returns (repo, lookback) pairs for all projects with a `github-ci` workflow.
    /// Lookback defaults to `"24h"` when not specified.
    pub fn github_ci_repos(&self) -> Vec<(String, String)> {
        self.projects
            .iter()
            .filter_map(|p| {
                p.workflow.iter().find_map(|w| {
                    if let toml::WorkflowConfig::GithubCi { lookback } = w {
                        let lb = lookback.clone().unwrap_or_else(|| "24h".into());
                        Some((p.repo.clone(), lb))
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    pub fn private_monitor_workflow_names(&self) -> Vec<String> {
        self.monitor
            .as_ref()
            .map(|m| m.workflow.iter().map(|w| w.name.clone()).collect())
            .unwrap_or_default()
    }

    /// Returns one `LokiEnv` per environment that has a `loki_endpoint` and at least
    /// one `loki-logs` workflow. Lookback defaults to `"1h"` and threshold to `10`.
    pub fn loki_envs(&self) -> Vec<domain::LokiEnv> {
        self.projects
            .iter()
            .flat_map(|p| {
                p.environment.iter().filter_map(|env| {
                    let endpoint = env.loki_endpoint.clone()?;
                    let queries: Vec<domain::LokiQuery> = env
                        .workflow
                        .iter()
                        .filter_map(|w| {
                            if let toml::WorkflowConfig::LokiLogs {
                                title,
                                query,
                                lookback,
                                error_threshold,
                            } = w
                            {
                                Some(domain::LokiQuery {
                                    title: title.clone(),
                                    query: query.clone(),
                                    lookback: lookback.clone().unwrap_or_else(|| "1h".into()),
                                    threshold: error_threshold.unwrap_or(10),
                                })
                            } else {
                                None
                            }
                        })
                        .collect();
                    if queries.is_empty() {
                        return None;
                    }
                    Some(domain::LokiEnv {
                        project: p.name.clone(),
                        env: env.env.clone(),
                        endpoint,
                        token: self.loki_token.clone(),
                        grafana_url: env.grafana_url.clone(),
                        queries,
                    })
                })
            })
            .collect()
    }

    /// Returns `(name, repo)` pairs for all projects opted into the `implement-issue` workflow.
    pub fn implement_repos(&self) -> Vec<(String, String)> {
        self.projects
            .iter()
            .filter(|p| {
                p.workflow
                    .iter()
                    .any(|w| matches!(w, toml::WorkflowConfig::ImplementIssue))
            })
            .map(|p| (p.name.clone(), p.repo.clone()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str, repo: &str, workflows: Vec<toml::WorkflowConfig>) -> toml::Project {
        toml::Project {
            name: name.into(),
            repo: repo.into(),
            workflow: workflows,
            environment: vec![],
        }
    }

    fn config(projects: Vec<toml::Project>) -> Config {
        Config {
            github_token: "tok".into(),
            github_username: "user".into(),
            linear_token: None,
            loki_token: None,
            projects,
            monitor: None,
        }
    }

    #[test]
    fn implement_repos_returns_opted_in_projects() {
        let cfg = config(vec![
            project(
                "hub",
                "ooloth/hub",
                vec![toml::WorkflowConfig::ImplementIssue],
            ),
            project(
                "other",
                "ooloth/other",
                vec![toml::WorkflowConfig::ImplementIssue],
            ),
        ]);
        assert_eq!(
            cfg.implement_repos(),
            vec![
                ("hub".to_string(), "ooloth/hub".to_string()),
                ("other".to_string(), "ooloth/other".to_string()),
            ]
        );
    }

    #[test]
    fn implement_repos_excludes_projects_without_workflow() {
        let cfg = config(vec![project(
            "hub",
            "ooloth/hub",
            vec![toml::WorkflowConfig::GithubPrs {
                exclude_authors: vec![],
            }],
        )]);
        assert!(cfg.implement_repos().is_empty());
    }

    #[test]
    fn implement_repos_empty_when_no_projects() {
        assert!(config(vec![]).implement_repos().is_empty());
    }
}
