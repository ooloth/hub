pub mod toml;

use anyhow::{Context, Result};
use secrecy::Secret;
use std::collections::HashMap;

pub struct Config {
    pub github_token: Secret<String>,
    pub github_username: String,
    pub linear_token: Option<Secret<String>>,
    pub loki_token: Option<Secret<String>>,
    pub extra_credentials: HashMap<String, Secret<String>>,
    pub projects: Vec<toml::Project>,
    pub monitor: Option<toml::Monitor>,
}

impl Config {
    pub async fn load() -> Result<Self> {
        let hub_toml = toml::parse_file("hub.toml")?;
        let creds = hub_toml.credentials;

        validate_required(&creds)?;

        let github_token = Secret::new(resolve(creds.github_token).await?);
        let linear_token = match creds.linear_token {
            Some(v) => Some(Secret::new(resolve(v).await?)),
            None => None,
        };
        let loki_token = match creds.loki_token {
            Some(v) => Some(Secret::new(resolve(v).await?)),
            None => None,
        };
        let mut extra_credentials = HashMap::new();
        for (k, v) in creds.extra {
            extra_credentials.insert(k, Secret::new(resolve(v).await?));
        }

        Ok(Self {
            github_token,
            github_username: creds.github_username,
            linear_token,
            loki_token,
            extra_credentials,
            projects: hub_toml.project,
            monitor: hub_toml.monitor,
        })
    }

    pub fn github_pr_repos(&self) -> Vec<domain::GithubPrsRepo> {
        self.projects
            .iter()
            .filter_map(|p| {
                p.workflow.iter().find_map(|w| {
                    if let toml::WorkflowConfig::GithubPrs { exclude_authors } = w {
                        Some(domain::GithubPrsRepo {
                            repo: p.repo.clone(),
                            exclude_authors: exclude_authors.clone(),
                        })
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    pub fn github_issue_repos(&self) -> Vec<String> {
        self.projects
            .iter()
            .filter_map(|p| {
                p.workflow
                    .iter()
                    .find(|w| matches!(w, toml::WorkflowConfig::GithubIssues {}))
                    .map(|_| p.repo.clone())
            })
            .collect()
    }

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

fn validate_required(creds: &toml::CredentialsToml) -> Result<()> {
    if creds.github_token.is_empty() {
        anyhow::bail!("credentials.github_token is required in hub.toml");
    }
    if creds.github_username.is_empty() {
        anyhow::bail!("credentials.github_username is required in hub.toml");
    }
    Ok(())
}

async fn resolve(value: String) -> Result<String> {
    if !value.starts_with("op://") {
        return Ok(value);
    }
    let output = tokio::process::Command::new("op")
        .args(["read", &value])
        .output()
        .await
        .context("failed to run `op read` — is the 1Password CLI installed?")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "failed to resolve credential from 1Password: {}",
            stderr.trim()
        );
    }
    let resolved = String::from_utf8(output.stdout).context("op read returned non-UTF-8 output")?;
    Ok(resolved.trim().to_string())
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
        }
    }

    // validate_required

    #[test]
    fn validate_required_passes_with_both_fields_set() {
        let creds = toml::CredentialsToml {
            github_token: "tok".into(),
            github_username: "user".into(),
            linear_token: None,
            loki_token: None,
            extra: HashMap::new(),
        };
        assert!(validate_required(&creds).is_ok());
    }

    #[test]
    fn validate_required_fails_on_empty_github_token() {
        let creds = toml::CredentialsToml {
            github_token: String::new(),
            github_username: "user".into(),
            linear_token: None,
            loki_token: None,
            extra: HashMap::new(),
        };
        let err = validate_required(&creds).unwrap_err();
        assert!(err.to_string().contains("github_token"));
    }

    #[test]
    fn validate_required_fails_on_empty_github_username() {
        let creds = toml::CredentialsToml {
            github_token: "tok".into(),
            github_username: String::new(),
            linear_token: None,
            loki_token: None,
            extra: HashMap::new(),
        };
        let err = validate_required(&creds).unwrap_err();
        assert!(err.to_string().contains("github_username"));
    }

    // resolve

    #[tokio::test]
    async fn resolve_returns_plain_value_unchanged() {
        let result = resolve("plain-value".into()).await.unwrap();
        assert_eq!(result, "plain-value");
    }

    #[tokio::test]
    async fn resolve_returns_empty_string_unchanged() {
        let result = resolve(String::new()).await.unwrap();
        assert_eq!(result, "");
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
