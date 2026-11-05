use crate::config_file::ConfigFile;
use crate::op_responsibility::OpResponsibility;
use crate::toml;
use anyhow::{anyhow, Result};
use domain::op_spawn::OpSpawnsUsed;
use secrecy::Secret;
use std::collections::HashMap;

/// A [`Config`] and every way of starting `op` that answered for its credentials.
#[derive(Debug)]
pub struct Resolved {
    /// The configuration, every credential resolved.
    pub config: Config,
    /// How each `op` that answered was started; empty when every credential was a plain value.
    pub spawns: OpSpawnsUsed,
}

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
    pub projects: crate::Projects,
    /// Optional monitor configuration for polling workflows.
    pub monitor: Option<toml::Monitor>,
    /// The MCP servers investigations on this device get, from `[investigation.mcp_servers]`.
    pub investigation_mcp_servers: domain::InvestigationMcpServers,
}

impl Config {
    /// Reads `hub.toml` and resolves every credential in it, through an `op` that answers to
    /// whatever is responsible for hub, as a terminal is for the TUI.
    ///
    /// # Errors
    /// Returns an error if `hub.toml` is missing or malformed, or if any credential
    /// reference (e.g. an `op://` URI) cannot be resolved.
    pub async fn load() -> Result<Self> {
        ConfigFile::read()?
            .resolve(OpResponsibility::Inherited)
            .await
            .map(|resolved| resolved.config)
            .map_err(|failure| anyhow!("{}", failure.reason()))
    }

    /// Returns repos that have the `github-prs` workflow configured.
    #[must_use]
    pub fn github_pr_repos(&self) -> Vec<domain::GithubPrsRepo> {
        self.projects
            .as_slice()
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
            .as_slice()
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
            .as_slice()
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
            .as_slice()
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
            .as_slice()
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
    use rstest::rstest;

    fn project(name: &str, repo: &str, workflows: Vec<toml::WorkflowConfig>) -> toml::Project {
        toml::Project {
            name: Some(name.into()),
            short_name: None,
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
            environment: environments,
            ..project(name, repo, vec![])
        }
    }

    /// An environment with neither a Loki endpoint nor a GCP project.
    fn env_without_a_log_source(workflows: Vec<toml::WorkflowConfig>) -> toml::Environment {
        toml::Environment {
            env: "prod".into(),
            gcp_project: None,
            gcp_region: None,
            loki_endpoint: None,
            grafana_url: None,
            workflow: workflows,
        }
    }

    fn loki_env(endpoint: &str, workflows: Vec<toml::WorkflowConfig>) -> toml::Environment {
        toml::Environment {
            loki_endpoint: Some(endpoint.into()),
            ..env_without_a_log_source(workflows)
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
            ..env_without_a_log_source(workflows)
        }
    }

    fn loki_logs(lookback: Option<&str>) -> toml::WorkflowConfig {
        toml::WorkflowConfig::LokiLogs {
            title: "errors".into(),
            query: "{app=\"myapp\"}".into(),
            lookback: lookback.map(Into::into),
            message_field: None,
        }
    }

    fn gcp_logs(lookback: Option<&str>) -> toml::WorkflowConfig {
        toml::WorkflowConfig::GcpLogs {
            title: "errors".into(),
            query: "severity>=ERROR".into(),
            lookback: lookback.map(Into::into),
            message_field: None,
        }
    }

    fn github_prs() -> toml::WorkflowConfig {
        toml::WorkflowConfig::GithubPrs {
            exclude_authors: vec![],
        }
    }

    fn config(projects: Vec<toml::Project>) -> Config {
        Config {
            github_token: Secret::new("tok".into()),
            github_username: "user".into(),
            linear_token: None,
            loki_token: None,
            extra_credentials: HashMap::new(),
            projects: crate::Projects::resolve(projects).unwrap(),
            monitor: None,
            investigation_mcp_servers: domain::InvestigationMcpServers::default(),
        }
    }

    // github_pr_repos

    #[test]
    fn github_pr_repos_returns_opted_in_repos() {
        let cfg = config(vec![
            project("hub", "ooloth/hub", vec![github_prs()]),
            project("other", "ooloth/other", vec![github_prs()]),
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
                    exclude_authors: vec!["dependabot-preview".into()],
                }],
            ),
            project("hub", "ooloth/hub", vec![github_prs()]),
        ]);
        let repos = cfg.github_pr_repos();
        assert_eq!(repos[0].exclude_authors, vec!["dependabot-preview"]);
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
        assert_eq!(cfg.github_issue_repos(), Vec::<String>::new());
    }

    // github_ci_repos

    #[rstest]
    #[case::specified(Some("48h"), "48h")]
    #[case::defaults_to_24h(None, "24h")]
    fn github_ci_repos_carry_the_lookback(
        #[case] configured: Option<&str>,
        #[case] expected: &str,
    ) {
        let cfg = config(vec![project(
            "hub",
            "ooloth/hub",
            vec![toml::WorkflowConfig::GithubCi {
                lookback: configured.map(Into::into),
            }],
        )]);
        assert_eq!(
            cfg.github_ci_repos(),
            vec![("ooloth/hub".to_string(), expected.to_string())]
        );
    }

    #[test]
    fn github_ci_repos_excludes_projects_without_workflow() {
        let cfg = config(vec![project("hub", "ooloth/hub", vec![github_prs()])]);
        assert_eq!(cfg.github_ci_repos(), Vec::<(String, String)>::new());
    }

    // private_monitor_workflow_names

    #[test]
    fn private_monitor_workflow_names_returns_names_when_monitor_set() {
        let cfg = Config {
            monitor: Some(toml::Monitor {
                workflow: vec![
                    toml::MonitorWorkflowConfig {
                        name: "github-prs".into(),
                    },
                    toml::MonitorWorkflowConfig {
                        name: "private-workflow".into(),
                    },
                ],
            }),
            ..config(vec![])
        };
        assert_eq!(
            cfg.private_monitor_workflow_names(),
            vec!["github-prs", "private-workflow"]
        );
    }

    #[test]
    fn private_monitor_workflow_names_empty_when_no_monitor() {
        assert_eq!(
            config(vec![]).private_monitor_workflow_names(),
            Vec::<String>::new()
        );
    }

    // loki_envs

    #[test]
    fn loki_envs_returns_env_with_endpoint_and_loki_logs() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![loki_env("https://loki.example.com", vec![loki_logs(None)])],
        )]);
        let envs = cfg.loki_envs();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].project, "myapp");
        assert_eq!(envs[0].endpoint, "https://loki.example.com");
        assert_eq!(envs[0].queries[0].title, "errors");
    }

    /// The clone an alert investigation runs in is found by repo, so the
    /// repo has to reach the alert even when the project's name differs.
    #[test]
    fn loki_envs_carry_the_project_repo_not_its_name() {
        let cfg = config(vec![project_with_envs(
            "My App",
            "org/myapp",
            vec![loki_env("https://loki.example.com", vec![loki_logs(None)])],
        )]);
        assert_eq!(cfg.loki_envs()[0].repo.to_string(), "org/myapp");
    }

    #[rstest]
    #[case::specified(Some("30m"), "30m")]
    #[case::defaults_to_1h(None, "1h")]
    fn loki_envs_carry_the_lookback(#[case] configured: Option<&str>, #[case] expected: &str) {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![loki_env(
                "https://loki.example.com",
                vec![loki_logs(configured)],
            )],
        )]);
        assert_eq!(cfg.loki_envs()[0].queries[0].lookback, expected);
    }

    #[test]
    fn loki_envs_skips_env_without_loki_endpoint() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![env_without_a_log_source(vec![loki_logs(None)])],
        )]);
        assert!(cfg.loki_envs().is_empty());
    }

    #[test]
    fn loki_envs_skips_env_with_endpoint_but_no_loki_logs_workflow() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![loki_env("https://loki.example.com", vec![])],
        )]);
        assert!(cfg.loki_envs().is_empty());
    }

    // gcp_envs

    #[test]
    fn gcp_envs_returns_env_with_gcp_project_and_gcp_logs() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![gcp_env("my-org-prod", None, vec![gcp_logs(None)])],
        )]);
        let envs = cfg.gcp_envs();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].project, "myapp");
        assert_eq!(envs[0].env, "neuro");
        assert_eq!(envs[0].gcp_project, "my-org-prod");
        assert_eq!(envs[0].queries[0].title, "errors");
    }

    #[test]
    fn gcp_envs_carry_the_project_repo_not_its_name() {
        let cfg = config(vec![project_with_envs(
            "My App",
            "org/myapp",
            vec![gcp_env("my-org-prod", None, vec![gcp_logs(None)])],
        )]);
        assert_eq!(cfg.gcp_envs()[0].repo.to_string(), "org/myapp");
    }

    #[rstest]
    #[case::specified(Some("30m"), "30m")]
    #[case::defaults_to_1h(None, "1h")]
    fn gcp_envs_carry_the_lookback(#[case] configured: Option<&str>, #[case] expected: &str) {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![gcp_env("my-org-prod", None, vec![gcp_logs(configured)])],
        )]);
        assert_eq!(cfg.gcp_envs()[0].queries[0].lookback, expected);
    }

    #[test]
    fn gcp_envs_propagates_gcp_region() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![gcp_env(
                "my-org-prod",
                Some("us-central1"),
                vec![gcp_logs(None)],
            )],
        )]);
        assert_eq!(cfg.gcp_envs()[0].gcp_region.as_deref(), Some("us-central1"));
    }

    #[test]
    fn gcp_envs_skips_env_without_gcp_project() {
        let cfg = config(vec![project_with_envs(
            "myapp",
            "org/myapp",
            vec![env_without_a_log_source(vec![gcp_logs(None)])],
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
                    gcp_logs(None),
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
