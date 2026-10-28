use crate::config_file::ConfigFile;
use crate::toml;
use anyhow::{anyhow, Result};
use secrecy::Secret;
use std::collections::HashMap;

/// Resolved runtime configuration, with all credential references expanded.
#[derive(Debug)]
pub struct Config {
    /// GitHub personal access token used for API calls.
    pub github_token: Secret<String>,
    /// GitHub username associated with the token.
    pub github_username: String,
    /// Optional Linear API token for issue tracking.
    pub linear_token: Option<Secret<String>>,
    /// Optional Loki API token for log querying.
    pub loki_token: Option<Secret<String>>,
    /// Additional named credentials for private integrations.
    pub extra_credentials: HashMap<String, Secret<String>>,
    /// Projects tracked in hub, sourced from `[[project]]` entries in `hub.toml`.
    pub projects: Vec<toml::Project>,
    /// Optional monitor configuration for polling workflows.
    pub monitor: Option<toml::Monitor>,
    /// The MCP servers investigations on this device get, from `[investigation.mcp_servers]`.
    pub investigation_mcp_servers: domain::InvestigationMcpServers,
}

impl Config {
    /// Reads `hub.toml` and resolves every credential in it.
    ///
    /// # Errors
    /// Returns an error if `hub.toml` is missing or malformed, or if any credential
    /// reference (e.g. an `op://` URI) cannot be resolved.
    pub async fn load() -> Result<Self> {
        ConfigFile::read()?
            .resolve()
            .await
            .map_err(|failure| anyhow!("{}", failure.reason()))
    }

    /// Returns repos that have the `github-prs` workflow configured.
    #[must_use]
    pub fn github_pr_repos(&self) -> Vec<domain::GithubPrsRepo> {
        self.projects
            .iter()
            .filter_map(|p| {
                p.workflow.iter().find_map(|w| {
                    if let toml::WorkflowConfig::GithubPrs { exclude_authors } = w {
                        Some(domain::GithubPrsRepo {
                            repo: p.repo.to_string(),
                            exclude_authors: exclude_authors.clone(),
                        })
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// Returns repo slugs that have the `github-issues` workflow configured.
    #[must_use]
    pub fn github_issue_repos(&self) -> Vec<String> {
        self.projects
            .iter()
            .filter_map(|p| {
                p.workflow
                    .iter()
                    .find(|w| matches!(w, toml::WorkflowConfig::GithubIssues {}))
                    .map(|_| p.repo.to_string())
            })
            .collect()
    }

    /// Returns `(repo_slug, lookback)` pairs for repos with the `github-ci` workflow.
    #[must_use]
    pub fn github_ci_repos(&self) -> Vec<(String, String)> {
        self.projects
            .iter()
            .filter_map(|p| {
                p.workflow.iter().find_map(|w| {
                    if let toml::WorkflowConfig::GithubCi { lookback } = w {
                        let lb = lookback.clone().unwrap_or_else(|| "24h".into());
                        Some((p.repo.to_string(), lb))
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// Returns the names of workflows listed under `[monitor]` in `hub.toml`.
    #[must_use]
    pub fn private_monitor_workflow_names(&self) -> Vec<String> {
        self.monitor
            .as_ref()
            .map(|m| m.workflow.iter().map(|w| w.name.clone()).collect())
            .unwrap_or_default()
    }

    /// Returns all Loki environments derived from project environment configurations.
    #[must_use]
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
                                message_field,
                            } = w
                            {
                                Some(domain::LokiQuery {
                                    title: title.clone(),
                                    query: query.clone(),
                                    lookback: lookback.clone().unwrap_or_else(|| "1h".into()),
                                    message_field: message_field
                                        .clone()
                                        .unwrap_or_else(|| "message".into()),
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
                        repo: p.repo.clone(),
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

    /// Returns all GCP environments derived from project environment configurations.
    #[must_use]
    pub fn gcp_envs(&self) -> Vec<domain::GcpEnv> {
        self.projects
            .iter()
            .flat_map(|p| {
                p.environment.iter().filter_map(|env| {
                    let gcp_project = env.gcp_project.clone()?;
                    let queries: Vec<domain::GcpQuery> = env
                        .workflow
                        .iter()
                        .filter_map(|w| {
                            if let toml::WorkflowConfig::GcpLogs {
                                title,
                                query,
                                lookback,
                                message_field,
                            } = w
                            {
                                Some(domain::GcpQuery {
                                    title: title.clone(),
                                    query: query.clone(),
                                    lookback: lookback.clone().unwrap_or_else(|| "1h".into()),
                                    message_field: message_field
                                        .clone()
                                        .unwrap_or_else(|| "message".into()),
                                })
                            } else {
                                None
                            }
                        })
                        .collect();
                    if queries.is_empty() {
                        return None;
                    }
                    Some(domain::GcpEnv {
                        project: p.name.clone(),
                        repo: p.repo.clone(),
                        env: env.env.clone(),
                        gcp_project,
                        gcp_region: env.gcp_region.clone(),
                        queries,
                    })
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
            repo: repo.parse().unwrap(),
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
            repo: repo.parse().unwrap(),
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
            loki_endpoint: Some(endpoint.into()),
            grafana_url: grafana_url.map(Into::into),
            workflow: workflows,
        }
    }

    fn gcp_env(
        gcp_project: &str,
        gcp_region: Option<&str>,
        workflows: Vec<toml::WorkflowConfig>,
    ) -> toml::Environment {
        toml::Environment {
            env: "neuro".into(),
            gcp_project: Some(gcp_project.into()),
            gcp_region: gcp_region.map(Into::into),
            loki_endpoint: None,
            grafana_url: None,
            workflow: workflows,
        }
    }

    fn env_no_loki(workflows: Vec<toml::WorkflowConfig>) -> toml::Environment {
        toml::Environment {
            env: "prod".into(),
            gcp_project: None,
            gcp_region: None,
            loki_endpoint: None,
            grafana_url: None,
            workflow: workflows,
        }
    }

    fn config(projects: Vec<toml::Project>) -> Config {
        Config {
            github_token: Secret::new("tok".into()),
            github_username: "user".into(),
            linear_token: None,
            loki_token: None,
            extra_credentials: HashMap::new(),
            projects,
            monitor: None,
            investigation_mcp_servers: domain::InvestigationMcpServers::default(),
        }
    }

    fn config_with_monitor(projects: Vec<toml::Project>, monitor: toml::Monitor) -> Config {
        Config {
            github_token: Secret::new("tok".into()),
            github_username: "user".into(),
            linear_token: None,
            loki_token: None,
            extra_credentials: HashMap::new(),
            projects,
            monitor: Some(monitor),
            investigation_mcp_servers: domain::InvestigationMcpServers::default(),
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
        let repos = cfg.github_pr_repos();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].repo, "ooloth/hub");
        assert_eq!(repos[1].repo, "ooloth/other");
    }

    #[test]
    fn github_pr_repos_preserves_exclude_authors_per_repo() {
        let cfg = config(vec![
            project(
                "gatsby",
                "ooloth/gatsbytutorials.com",
                vec![toml::WorkflowConfig::GithubPrs {
                    exclude_authors: vec!["dependabot-preview[bot]".into()],
                }],
            ),
            project(
                "hub",
                "ooloth/hub",
                vec![toml::WorkflowConfig::GithubPrs {
                    exclude_authors: vec![],
                }],
            ),
        ]);
        let repos = cfg.github_pr_repos();
        assert_eq!(repos[0].exclude_authors, vec!["dependabot-preview[bot]"]);
        assert_eq!(repos[1].exclude_authors, Vec::<String>::new());
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

    // github_issue_repos

    #[test]
    fn github_issue_repos_returns_repos_with_github_issues_workflow() {
        let cfg = config(vec![project(
            "hub",
            "ooloth/hub",
            vec![toml::WorkflowConfig::GithubIssues {}],
        )]);
        assert_eq!(cfg.github_issue_repos(), vec!["ooloth/hub"]);
    }

    #[test]
    fn github_issue_repos_excludes_projects_without_github_issues_workflow() {
        let cfg = config(vec![project(
            "hub",
            "ooloth/hub",
            vec![toml::WorkflowConfig::GithubCi { lookback: None }],
        )]);
        assert!(cfg.github_issue_repos().is_empty());
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
                    message_field: None,
                }],
            )],
        )]);
        let envs = cfg.loki_envs();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].project, "myapp");
        assert_eq!(envs[0].endpoint, "https://loki.example.com");
        assert_eq!(envs[0].queries[0].title, "errors");
        assert_eq!(envs[0].queries[0].lookback, "30m");
    }

    /// The clone an alert investigation runs in is found by repo, so the
    /// repo has to reach the alert even when the project's name differs.
    #[test]
    fn loki_envs_carry_the_project_repo_not_its_name() {
        let cfg = config(vec![project_with_envs(
            "My App",
            "org/myapp",
            vec![loki_env(
                "https://loki.example.com",
                None,
                vec![toml::WorkflowConfig::LokiLogs {
                    title: "errors".into(),
                    query: "{app=\"myapp\"}".into(),
                    lookback: None,
                    message_field: None,
                }],
            )],
        )]);
        assert_eq!(cfg.loki_envs()[0].repo.to_string(), "org/myapp");
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
                    message_field: None,
                }],
            )],
        )]);
        assert_eq!(cfg.loki_envs()[0].queries[0].lookback, "1h");
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
                message_field: None,
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

    // gcp_envs

    #[test]
    fn gcp_envs_returns_env_with_gcp_project_and_gcp_logs() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![gcp_env(
                "my-org-prod",
                None,
                vec![toml::WorkflowConfig::GcpLogs {
                    title: "errors".into(),
                    query: "severity>=ERROR".into(),
                    lookback: Some("30m".into()),
                    message_field: None,
                }],
            )],
        )]);
        let envs = cfg.gcp_envs();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].project, "myapp");
        assert_eq!(envs[0].env, "neuro");
        assert_eq!(envs[0].gcp_project, "my-org-prod");
        assert_eq!(envs[0].queries[0].title, "errors");
        assert_eq!(envs[0].queries[0].lookback, "30m");
    }

    #[test]
    fn gcp_envs_carry_the_project_repo_not_its_name() {
        let cfg = config(vec![project_with_envs(
            "My App",
            "org/myapp",
            vec![gcp_env(
                "my-org-prod",
                None,
                vec![toml::WorkflowConfig::GcpLogs {
                    title: "errors".into(),
                    query: "severity>=ERROR".into(),
                    lookback: None,
                    message_field: None,
                }],
            )],
        )]);
        assert_eq!(cfg.gcp_envs()[0].repo.to_string(), "org/myapp");
    }

    #[test]
    fn gcp_envs_defaults_lookback_to_1h() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![gcp_env(
                "my-org-prod",
                None,
                vec![toml::WorkflowConfig::GcpLogs {
                    title: "errors".into(),
                    query: "severity>=ERROR".into(),
                    lookback: None,
                    message_field: None,
                }],
            )],
        )]);
        assert_eq!(cfg.gcp_envs()[0].queries[0].lookback, "1h");
    }

    #[test]
    fn gcp_envs_propagates_gcp_region() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![gcp_env(
                "my-org-prod",
                Some("us-central1"),
                vec![toml::WorkflowConfig::GcpLogs {
                    title: "errors".into(),
                    query: "severity>=ERROR".into(),
                    lookback: None,
                    message_field: None,
                }],
            )],
        )]);
        assert_eq!(cfg.gcp_envs()[0].gcp_region.as_deref(), Some("us-central1"));
    }

    #[test]
    fn gcp_envs_skips_env_without_gcp_project() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![env_no_loki(vec![toml::WorkflowConfig::GcpLogs {
                title: "errors".into(),
                query: "severity>=ERROR".into(),
                lookback: None,
                message_field: None,
            }])],
        )]);
        assert!(cfg.gcp_envs().is_empty());
    }

    #[test]
    fn gcp_envs_skips_env_with_gcp_project_but_no_gcp_logs_workflow() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![gcp_env("my-org-prod", None, vec![])],
        )]);
        assert!(cfg.gcp_envs().is_empty());
    }

    #[test]
    fn gcp_envs_collects_multiple_queries_on_one_env() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![gcp_env(
                "my-org-prod",
                None,
                vec![
                    toml::WorkflowConfig::GcpLogs {
                        title: "errors".into(),
                        query: "severity>=ERROR".into(),
                        lookback: None,
                        message_field: None,
                    },
                    toml::WorkflowConfig::GcpLogs {
                        title: "errors (external)".into(),
                        query: "severity>=ERROR AND labels.user_type=\"external\"".into(),
                        lookback: Some("30m".into()),
                        message_field: None,
                    },
                ],
            )],
        )]);
        let envs = cfg.gcp_envs();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].queries.len(), 2);
        assert_eq!(envs[0].queries[0].title, "errors");
        assert_eq!(envs[0].queries[1].title, "errors (external)");
    }
}
