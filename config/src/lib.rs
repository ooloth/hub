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

    fn project_with_envs(
        name: &str,
        repo: &str,
        environments: Vec<toml::Environment>,
    ) -> toml::Project {
        toml::Project {
            name: name.into(),
            repo: repo.into(),
            workflow: vec![],
            environment: environments,
        }
    }

    fn loki_env(
        endpoint: &str,
        grafana_url: Option<&str>,
        workflows: Vec<toml::WorkflowConfig>,
    ) -> toml::Environment {
        toml::Environment {
            env: "prod".into(),
            gcp_project: None,
            gcp_region: None,
            service: None,
            cluster: None,
            namespace: None,
            loki_endpoint: Some(endpoint.into()),
            grafana_url: grafana_url.map(Into::into),
            workflow: workflows,
        }
    }

    fn env_no_loki(workflows: Vec<toml::WorkflowConfig>) -> toml::Environment {
        toml::Environment {
            env: "prod".into(),
            gcp_project: None,
            gcp_region: None,
            service: None,
            cluster: None,
            namespace: None,
            loki_endpoint: None,
            grafana_url: None,
            workflow: workflows,
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

    fn config_with_monitor(projects: Vec<toml::Project>, monitor: toml::Monitor) -> Config {
        Config {
            github_token: "tok".into(),
            github_username: "user".into(),
            linear_token: None,
            loki_token: None,
            projects,
            monitor: Some(monitor),
        }
    }

    // github_pr_repos

    #[test]
    fn github_pr_repos_returns_opted_in_repos() {
        let cfg = config(vec![
            project(
                "hub",
                "ooloth/hub",
                vec![toml::WorkflowConfig::GithubPrs {
                    exclude_authors: vec![],
                }],
            ),
            project(
                "other",
                "ooloth/other",
                vec![toml::WorkflowConfig::GithubPrs {
                    exclude_authors: vec![],
                }],
            ),
        ]);
        assert_eq!(cfg.github_pr_repos(), vec!["ooloth/hub", "ooloth/other"]);
    }

    #[test]
    fn github_pr_repos_excludes_projects_without_workflow() {
        let cfg = config(vec![project(
            "hub",
            "ooloth/hub",
            vec![toml::WorkflowConfig::GithubCi { lookback: None }],
        )]);
        assert!(cfg.github_pr_repos().is_empty());
    }

    // github_open_issue_repos

    #[test]
    fn github_open_issue_repos_returns_repos_with_assigned_only_false() {
        let cfg = config(vec![project(
            "hub",
            "ooloth/hub",
            vec![toml::WorkflowConfig::GithubIssues {
                exclude_labels: vec![],
                assigned_only: false,
            }],
        )]);
        assert_eq!(cfg.github_open_issue_repos(), vec!["ooloth/hub"]);
    }

    #[test]
    fn github_open_issue_repos_excludes_assigned_only_repos() {
        let cfg = config(vec![project(
            "hub",
            "ooloth/hub",
            vec![toml::WorkflowConfig::GithubIssues {
                exclude_labels: vec![],
                assigned_only: true,
            }],
        )]);
        assert!(cfg.github_open_issue_repos().is_empty());
    }

    // github_assigned_issue_repos

    #[test]
    fn github_assigned_issue_repos_returns_repos_with_assigned_only_true() {
        let cfg = config(vec![project(
            "hub",
            "ooloth/hub",
            vec![toml::WorkflowConfig::GithubIssues {
                exclude_labels: vec![],
                assigned_only: true,
            }],
        )]);
        assert_eq!(cfg.github_assigned_issue_repos(), vec!["ooloth/hub"]);
    }

    #[test]
    fn github_assigned_issue_repos_excludes_open_issue_repos() {
        let cfg = config(vec![project(
            "hub",
            "ooloth/hub",
            vec![toml::WorkflowConfig::GithubIssues {
                exclude_labels: vec![],
                assigned_only: false,
            }],
        )]);
        assert!(cfg.github_assigned_issue_repos().is_empty());
    }

    // github_ci_repos

    #[test]
    fn github_ci_repos_uses_specified_lookback() {
        let cfg = config(vec![project(
            "hub",
            "ooloth/hub",
            vec![toml::WorkflowConfig::GithubCi {
                lookback: Some("48h".into()),
            }],
        )]);
        assert_eq!(
            cfg.github_ci_repos(),
            vec![("ooloth/hub".to_string(), "48h".to_string())]
        );
    }

    #[test]
    fn github_ci_repos_defaults_lookback_to_24h() {
        let cfg = config(vec![project(
            "hub",
            "ooloth/hub",
            vec![toml::WorkflowConfig::GithubCi { lookback: None }],
        )]);
        assert_eq!(
            cfg.github_ci_repos(),
            vec![("ooloth/hub".to_string(), "24h".to_string())]
        );
    }

    #[test]
    fn github_ci_repos_excludes_projects_without_workflow() {
        let cfg = config(vec![project(
            "hub",
            "ooloth/hub",
            vec![toml::WorkflowConfig::GithubPrs {
                exclude_authors: vec![],
            }],
        )]);
        assert!(cfg.github_ci_repos().is_empty());
    }

    // private_monitor_workflow_names

    #[test]
    fn private_monitor_workflow_names_returns_names_when_monitor_set() {
        let monitor = toml::Monitor {
            workflow: vec![
                toml::MonitorWorkflowConfig {
                    name: "github-prs".into(),
                },
                toml::MonitorWorkflowConfig {
                    name: "private-workflow".into(),
                },
            ],
        };
        let cfg = config_with_monitor(vec![], monitor);
        assert_eq!(
            cfg.private_monitor_workflow_names(),
            vec!["github-prs", "private-workflow"]
        );
    }

    #[test]
    fn private_monitor_workflow_names_empty_when_no_monitor() {
        assert!(config(vec![]).private_monitor_workflow_names().is_empty());
    }

    // loki_envs

    #[test]
    fn loki_envs_returns_env_with_endpoint_and_loki_logs() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![loki_env(
                "https://loki.example.com",
                None,
                vec![toml::WorkflowConfig::LokiLogs {
                    title: "errors".into(),
                    query: "{app=\"myapp\"}".into(),
                    lookback: Some("30m".into()),
                    error_threshold: Some(5),
                }],
            )],
        )]);
        let envs = cfg.loki_envs();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].project, "myapp");
        assert_eq!(envs[0].endpoint, "https://loki.example.com");
        assert_eq!(envs[0].queries[0].title, "errors");
        assert_eq!(envs[0].queries[0].lookback, "30m");
        assert_eq!(envs[0].queries[0].threshold, 5);
    }

    #[test]
    fn loki_envs_defaults_lookback_to_1h() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![loki_env(
                "https://loki.example.com",
                None,
                vec![toml::WorkflowConfig::LokiLogs {
                    title: "errors".into(),
                    query: "{app=\"myapp\"}".into(),
                    lookback: None,
                    error_threshold: Some(5),
                }],
            )],
        )]);
        assert_eq!(cfg.loki_envs()[0].queries[0].lookback, "1h");
    }

    #[test]
    fn loki_envs_defaults_threshold_to_10() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![loki_env(
                "https://loki.example.com",
                None,
                vec![toml::WorkflowConfig::LokiLogs {
                    title: "errors".into(),
                    query: "{app=\"myapp\"}".into(),
                    lookback: None,
                    error_threshold: None,
                }],
            )],
        )]);
        assert_eq!(cfg.loki_envs()[0].queries[0].threshold, 10);
    }

    #[test]
    fn loki_envs_skips_env_without_loki_endpoint() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![env_no_loki(vec![toml::WorkflowConfig::LokiLogs {
                title: "errors".into(),
                query: "{app=\"myapp\"}".into(),
                lookback: None,
                error_threshold: None,
            }])],
        )]);
        assert!(cfg.loki_envs().is_empty());
    }

    #[test]
    fn loki_envs_skips_env_with_endpoint_but_no_loki_logs_workflow() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![loki_env("https://loki.example.com", None, vec![])],
        )]);
        assert!(cfg.loki_envs().is_empty());
    }
}
