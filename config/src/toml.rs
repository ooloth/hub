use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct HubToml {
    pub credentials: CredentialsToml,
    #[serde(default)]
    pub project: Vec<Project>,
    pub monitor: Option<Monitor>,
}

/// Raw credential values from the `[credentials]` section of `hub.toml`.
/// `op://` references are resolved later during [`Config::load`].
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct CredentialsToml {
    /// GitHub personal access token (plain value or `op://` reference).
    pub github_token: String,
    /// GitHub username associated with the token.
    pub github_username: String,
    /// Optional Linear API token.
    pub linear_token: Option<String>,
    /// Optional Loki API token.
    pub loki_token: Option<String>,
    /// Additional named credentials for private integrations.
    #[serde(flatten)]
    pub extra: HashMap<String, String>,
}

/// A project tracked by hub, from a `[[project]]` entry in `hub.toml`.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Project {
    /// Human-readable project name shown in the TUI.
    pub name: String,
    /// GitHub repo slug in `owner/repo` form.
    pub repo: String,
    /// Workflows enabled at the repo level (e.g. PRs, CI).
    #[serde(default)]
    pub workflow: Vec<WorkflowConfig>,
    /// Per-environment configurations (e.g. prod, staging).
    #[serde(default)]
    pub environment: Vec<Environment>,
}

/// A deployment environment within a project, from `[[project.environment]]`.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Environment {
    /// Environment name (e.g. `prod`, `staging`).
    pub env: String,
    /// GCP project ID for Cloud Logging queries.
    pub gcp_project: Option<String>,
    /// GCP region used to scope log queries.
    pub gcp_region: Option<String>,
    /// Loki push/query endpoint URL.
    pub loki_endpoint: Option<String>,
    /// Grafana base URL used to generate deep-link URLs in alerts.
    pub grafana_url: Option<String>,
    /// Workflows enabled for this environment (e.g. log queries).
    #[serde(default)]
    pub workflow: Vec<WorkflowConfig>,
}

/// A workflow configuration entry, discriminated by the `name` field in TOML.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "name")]
pub enum WorkflowConfig {
    /// Poll GitHub Actions runs for a repo (`name = "github-ci"`).
    #[serde(rename = "github-ci")]
    GithubCi {
        /// How far back to look for runs (e.g. `"24h"`). Defaults to `"24h"`.
        lookback: Option<String>,
    },
    /// Poll open pull requests for a repo (`name = "github-prs"`).
    #[serde(rename = "github-prs")]
    GithubPrs {
        /// PR authors to hide from the results (e.g. bots).
        #[serde(default)]
        exclude_authors: Vec<String>,
    },
    /// Poll open GitHub issues for a repo (`name = "github-issues"`).
    #[serde(rename = "github-issues")]
    GithubIssues {},
    /// Query Google Cloud Logging (`name = "gcp-logs"`).
    #[serde(rename = "gcp-logs")]
    GcpLogs {
        /// Display title shown in the TUI for this query.
        title: String,
        /// Log filter query string.
        query: String,
        /// How far back to query (e.g. `"1h"`). Defaults to `"1h"`.
        lookback: Option<String>,
        /// JSON field name containing the log message. Defaults to `"message"`.
        message_field: Option<String>,
    },
    /// Query a Loki instance (`name = "loki-logs"`).
    #[serde(rename = "loki-logs")]
    LokiLogs {
        /// Display title shown in the TUI for this query.
        title: String,
        /// `LogQL` query string.
        query: String,
        /// How far back to query (e.g. `"1h"`). Defaults to `"1h"`.
        lookback: Option<String>,
        /// JSON field name containing the log message. Defaults to `"message"`.
        message_field: Option<String>,
    },
}

/// Monitor configuration from the `[monitor]` section of `hub.toml`.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Monitor {
    /// Workflows the monitor daemon should run on its polling cycle.
    #[serde(default)]
    pub workflow: Vec<MonitorWorkflowConfig>,
}

/// A single workflow entry under `[[monitor.workflow]]`.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct MonitorWorkflowConfig {
    /// Workflow name, matched against registered monitor handlers.
    pub name: String,
}

pub(crate) fn parse(content: &str) -> Result<HubToml> {
    toml::from_str(content).context("failed to parse hub.toml")
}

pub(crate) fn parse_file(path: &str) -> Result<HubToml> {
    let content = std::fs::read_to_string(path).with_context(|| {
        if std::path::Path::new(path)
            .try_exists()
            .unwrap_or(false)
        {
            format!("failed to read {path}")
        } else {
            format!("{path} not found — copy hub.toml.example to hub.toml and add a [credentials] section")
        }
    })?;
    parse(&content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rstest::rstest;

    const CREDENTIALS: &str = r#"
        [credentials]
        github_token = "test-token"
        github_username = "testuser"
    "#;

    fn with_credentials(rest: &str) -> String {
        format!("{CREDENTIALS}\n{rest}")
    }

    #[test]
    fn credentials_only_config_parses() {
        let result = parse(CREDENTIALS).unwrap();
        assert_eq!(result.credentials.github_token, "test-token");
        assert_eq!(result.credentials.github_username, "testuser");
        assert!(result.credentials.linear_token.is_none());
        assert!(result.credentials.loki_token.is_none());
        assert!(result.credentials.extra.is_empty());
        assert_eq!(result.project, vec![]);
        assert!(result.monitor.is_none());
    }

    #[test]
    fn credentials_with_all_fields_parse() {
        let result = parse(
            r#"
            [credentials]
            github_token = "tok"
            github_username = "user"
            linear_token = "lin"
            loki_token = "loki"
            service_url = "http://service.local"
            service_api_key = "key"
        "#,
        )
        .unwrap();
        assert_eq!(result.credentials.linear_token.as_deref(), Some("lin"));
        assert_eq!(result.credentials.loki_token.as_deref(), Some("loki"));
        // Unrecognized credential keys land in the `extra` catch-all map, so
        // private integrations resolve their own keys by name without a typed
        // field per service.
        assert_eq!(
            result
                .credentials
                .extra
                .get("service_url")
                .map(String::as_str),
            Some("http://service.local")
        );
        assert_eq!(
            result
                .credentials
                .extra
                .get("service_api_key")
                .map(String::as_str),
            Some("key")
        );
    }

    #[rstest]
    #[case("github-ci", WorkflowConfig::GithubCi { lookback: None })]
    #[case("github-issues", WorkflowConfig::GithubIssues {})]
    #[case("github-prs", WorkflowConfig::GithubPrs { exclude_authors: vec![] })]
    fn all_workflow_types_parse_with_name_only(
        #[case] name: &str,
        #[case] expected: WorkflowConfig,
    ) {
        let toml = with_credentials(&format!(
            r#"
            [[project]]
            name = "hub"
            repo = "ooloth/hub"

            [[project.workflow]]
            name = "{name}"
        "#
        ));
        let result = parse(&toml).unwrap();
        assert_eq!(result.project[0].workflow, vec![expected]);
    }

    #[test]
    fn github_ci_with_lookback() {
        let result = parse(&with_credentials(
            r#"
            [[project]]
            name = "hub"
            repo = "ooloth/hub"

            [[project.workflow]]
            name = "github-ci"
            lookback = "48h"
        "#,
        ))
        .unwrap();
        assert_eq!(
            result.project[0].workflow,
            vec![WorkflowConfig::GithubCi {
                lookback: Some("48h".into()),
            }]
        );
    }

    #[test]
    fn github_prs_with_exclude_authors() {
        let result = parse(&with_credentials(
            r#"
            [[project]]
            name = "hub"
            repo = "ooloth/hub"

            [[project.workflow]]
            name = "github-prs"
            exclude_authors = ["dependabot", "renovate"]
        "#,
        ))
        .unwrap();
        assert_eq!(
            result.project[0].workflow,
            vec![WorkflowConfig::GithubPrs {
                exclude_authors: vec!["dependabot".into(), "renovate".into()],
            }]
        );
    }

    #[test]
    fn loki_logs_parses_with_required_fields_only() {
        let result = parse(&with_credentials(
            r#"
            [[project]]
            name = "myapp"
            repo = "org/myapp"

            [[project.environment]]
            env = "internal"
            loki_endpoint = "https://loki.example.com"

            [[project.environment.workflow]]
            name = "loki-logs"
            title = "app errors"
            query = '{app="myapp"} | logfmt | level="error"'
        "#,
        ))
        .unwrap();
        let env = &result.project[0].environment[0];
        assert_eq!(
            env.loki_endpoint.as_deref(),
            Some("https://loki.example.com")
        );
        assert_eq!(
            env.workflow,
            vec![WorkflowConfig::LokiLogs {
                title: "app errors".into(),
                query: "{app=\"myapp\"} | logfmt | level=\"error\"".into(),
                lookback: None,
                message_field: None,
            }]
        );
    }

    #[test]
    fn loki_logs_parses_with_lookback() {
        let result = parse(&with_credentials(
            r#"
            [[project]]
            name = "myapp"
            repo = "org/myapp"

            [[project.environment]]
            env = "internal"
            loki_endpoint = "https://loki.example.com"

            [[project.environment.workflow]]
            name = "loki-logs"
            title = "worker panics"
            query = '{app="myapp",component="worker"} |= "panic"'
            lookback = "30m"
        "#,
        ))
        .unwrap();
        assert_eq!(
            result.project[0].environment[0].workflow,
            vec![WorkflowConfig::LokiLogs {
                title: "worker panics".into(),
                query: r#"{app="myapp",component="worker"} |= "panic""#.into(),
                lookback: Some("30m".into()),
                message_field: None,
            }]
        );
    }

    #[test]
    fn gcp_logs_parses_with_required_fields_only() {
        let result = parse(&with_credentials(
            r#"
            [[project]]
            name = "myapp"
            repo = "org/myapp"

            [[project.environment]]
            env = "neuro"
            gcp_project = "my-org-prod"

            [[project.environment.workflow]]
            name = "gcp-logs"
            title = "errors"
            query = 'resource.type="cloud_run_revision" AND severity>=ERROR'
        "#,
        ))
        .unwrap();
        let env = &result.project[0].environment[0];
        assert_eq!(env.gcp_project.as_deref(), Some("my-org-prod"));
        assert_eq!(
            env.workflow,
            vec![WorkflowConfig::GcpLogs {
                title: "errors".into(),
                query: "resource.type=\"cloud_run_revision\" AND severity>=ERROR".into(),
                lookback: None,
                message_field: None,
            }]
        );
    }

    #[test]
    fn gcp_logs_parses_with_lookback() {
        let result = parse(&with_credentials(
            r#"
            [[project]]
            name = "myapp"
            repo = "org/myapp"

            [[project.environment]]
            env = "neuro"
            gcp_project = "my-org-prod"
            gcp_region = "us-central1"

            [[project.environment.workflow]]
            name = "gcp-logs"
            title = "errors"
            query = 'resource.type="cloud_run_revision" AND severity>=ERROR'
            lookback = "30m"
        "#,
        ))
        .unwrap();
        assert_eq!(
            result.project[0].environment[0].workflow,
            vec![WorkflowConfig::GcpLogs {
                title: "errors".into(),
                query: "resource.type=\"cloud_run_revision\" AND severity>=ERROR".into(),
                lookback: Some("30m".into()),
                message_field: None,
            }]
        );
    }

    #[test]
    fn monitor_with_known_workflow_name() {
        let result = parse(&with_credentials(
            r#"
            [[monitor.workflow]]
            name = "github-prs"
        "#,
        ))
        .unwrap();
        let monitor = result.monitor.unwrap();
        assert_eq!(
            monitor.workflow,
            vec![MonitorWorkflowConfig {
                name: "github-prs".into()
            }]
        );
    }

    #[test]
    fn monitor_with_unknown_workflow_name_does_not_error() {
        let result = parse(&with_credentials(
            r#"
            [[monitor.workflow]]
            name = "private-integration"
        "#,
        ))
        .unwrap();
        let monitor = result.monitor.unwrap();
        assert_eq!(
            monitor.workflow,
            vec![MonitorWorkflowConfig {
                name: "private-integration".into()
            }]
        );
    }

    #[test]
    fn unknown_workflow_name_is_an_error() {
        let err = parse(&with_credentials(
            r#"
            [[project]]
            name = "hub"
            repo = "ooloth/hub"

            [[project.workflow]]
            name = "nonexistent-workflow"
        "#,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("failed to parse hub.toml"));
    }

    #[test]
    fn missing_file_returns_error() {
        let err = parse_file("/nonexistent/path/hub.toml").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn project_no_workflows() {
        let result = parse(&with_credentials(
            r#"
            [[project]]
            name = "hub"
            repo = "ooloth/hub"
        "#,
        ))
        .unwrap();
        assert_eq!(result.project.len(), 1);
        assert_eq!(result.project[0].name, "hub");
        assert_eq!(result.project[0].repo, "ooloth/hub");
        assert_eq!(result.project[0].workflow, vec![]);
        assert_eq!(result.project[0].environment, vec![]);
    }

    #[test]
    fn snapshot_full_device_config() {
        let result = parse(
            r#"
            [credentials]
            github_token = "op://Scripts/GitHub/token"
            github_username = "testuser"
            linear_token = "op://Scripts/Linear/key"

            [[project]]
            name = "myapp"
            repo = "org/myapp"

            [[project.workflow]]
            name = "github-prs"
            exclude_authors = ["dependabot"]

            [[project.workflow]]
            name = "github-issues"

            [[project.environment]]
            env = "internal"
            loki_endpoint = "https://loki.example.com"
            grafana_url = "https://grafana.example.com"

            [[project.environment.workflow]]
            name = "loki-logs"
            title = "app errors"
            query = '{app="myapp"} | logfmt | level="error"'
            lookback = "1h"

            [[project.environment]]
            env = "neuro"
            gcp_project = "org-prod"
            gcp_region = "us-central1"

            [[project.environment.workflow]]
            name = "gcp-logs"
            title = "errors"
            query = 'resource.type="cloud_run_revision" AND severity>=ERROR'

            [[project.environment.workflow]]
            name = "gcp-logs"
            title = "errors (external)"
            query = 'resource.type="cloud_run_revision" AND severity>=ERROR AND labels."user-type"="external"'
            lookback = "30m"

            [[monitor.workflow]]
            name = "github-prs"
        "#,
        )
        .unwrap();
        insta::assert_debug_snapshot!(result);
    }

    proptest! {
        #[test]
        fn project_fields_round_trip(
            name in "[a-zA-Z][a-zA-Z0-9_. -]{0,30}",
            owner in "[a-zA-Z][a-zA-Z0-9_-]{0,15}",
            repo in "[a-zA-Z][a-zA-Z0-9_-]{0,15}",
        ) {
            let toml = format!(
                "[credentials]\ngithub_token = \"tok\"\ngithub_username = \"user\"\n\n[[project]]\nname = \"{name}\"\nrepo = \"{owner}/{repo}\"\n"
            );
            let result = parse(&toml).unwrap();
            prop_assert_eq!(&result.project[0].name, &name);
            prop_assert_eq!(&result.project[0].repo, &format!("{owner}/{repo}"));
        }
    }
}
