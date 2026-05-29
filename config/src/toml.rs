use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct HubToml {
    #[serde(default)]
    pub project: Vec<Project>,
    pub monitor: Option<Monitor>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Project {
    pub name: String,
    pub repo: String,
    #[serde(default)]
    pub workflow: Vec<WorkflowConfig>,
    #[serde(default)]
    pub environment: Vec<Environment>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Environment {
    pub env: String,
    pub gcp_project: Option<String>,
    pub gcp_region: Option<String>,
    pub loki_endpoint: Option<String>,
    pub grafana_url: Option<String>,
    #[serde(default)]
    pub workflow: Vec<WorkflowConfig>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "name")]
pub enum WorkflowConfig {
    #[serde(rename = "github-ci")]
    GithubCi { lookback: Option<String> },
    #[serde(rename = "github-prs")]
    GithubPrs {
        #[serde(default)]
        exclude_authors: Vec<String>,
    },
    #[serde(rename = "github-issues")]
    GithubIssues {},
    #[serde(rename = "gcp-logs")]
    GcpLogs {
        title: String,
        query: String,
        lookback: Option<String>,
        message_field: Option<String>,
    },
    #[serde(rename = "loki-logs")]
    LokiLogs {
        title: String,
        query: String,
        lookback: Option<String>,
        message_field: Option<String>,
    },
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Monitor {
    #[serde(default)]
    pub workflow: Vec<MonitorWorkflowConfig>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct MonitorWorkflowConfig {
    pub name: String,
}

pub(crate) fn parse(content: &str) -> Result<HubToml> {
    toml::from_str(content).context("failed to parse hub.toml")
}

pub(crate) fn parse_file(path: &str) -> Result<HubToml> {
    match std::fs::read_to_string(path) {
        Ok(content) => parse(&content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HubToml {
            project: vec![],
            monitor: None,
        }),
        Err(e) => Err(e).context(format!("failed to read {path}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rstest::rstest;

    #[test]
    fn empty_file() {
        let result = parse("").unwrap();
        assert_eq!(result.project, vec![]);
        assert!(result.monitor.is_none());
    }

    #[rstest]
    #[case("github-ci", WorkflowConfig::GithubCi { lookback: None })]
    #[case("github-issues", WorkflowConfig::GithubIssues {})]
    #[case("github-prs", WorkflowConfig::GithubPrs { exclude_authors: vec![] })]
    fn all_workflow_types_parse_with_name_only(
        #[case] name: &str,
        #[case] expected: WorkflowConfig,
    ) {
        let toml = format!(
            r#"
            [[project]]
            name = "hub"
            repo = "ooloth/hub"

            [[project.workflow]]
            name = "{name}"
        "#
        );
        let result = parse(&toml).unwrap();
        assert_eq!(result.project[0].workflow, vec![expected]);
    }

    #[test]
    fn github_ci_with_lookback() {
        let result = parse(
            r#"
            [[project]]
            name = "hub"
            repo = "ooloth/hub"

            [[project.workflow]]
            name = "github-ci"
            lookback = "48h"
        "#,
        )
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
        let result = parse(
            r#"
            [[project]]
            name = "hub"
            repo = "ooloth/hub"

            [[project.workflow]]
            name = "github-prs"
            exclude_authors = ["dependabot", "renovate"]
        "#,
        )
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
        let result = parse(
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
        )
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
        let result = parse(
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
        )
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
        let result = parse(
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
        )
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
        let result = parse(
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
        )
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
        let result = parse(
            r#"
            [[monitor.workflow]]
            name = "github-prs"
        "#,
        )
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
        let result = parse(
            r#"
            [[monitor.workflow]]
            name = "private-integration"
        "#,
        )
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
        let err = parse(
            r#"
            [[project]]
            name = "hub"
            repo = "ooloth/hub"

            [[project.workflow]]
            name = "nonexistent-workflow"
        "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("failed to parse hub.toml"));
    }

    #[test]
    fn missing_file_returns_empty() {
        let result = parse_file("/nonexistent/path/hub.toml").unwrap();
        assert_eq!(result.project, vec![]);
        assert!(result.monitor.is_none());
    }

    #[test]
    fn project_no_workflows() {
        let result = parse(
            r#"
            [[project]]
            name = "hub"
            repo = "ooloth/hub"
        "#,
        )
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
        // Arbitrary project names and repo slugs survive a parse round-trip.
        #[test]
        fn project_fields_round_trip(
            name in "[a-zA-Z][a-zA-Z0-9_. -]{0,30}",
            owner in "[a-zA-Z][a-zA-Z0-9_-]{0,15}",
            repo in "[a-zA-Z][a-zA-Z0-9_-]{0,15}",
        ) {
            let toml = format!(
                "[[project]]\nname = \"{name}\"\nrepo = \"{owner}/{repo}\"\n"
            );
            let result = parse(&toml).unwrap();
            prop_assert_eq!(&result.project[0].name, &name);
            prop_assert_eq!(&result.project[0].repo, &format!("{owner}/{repo}"));
        }
    }
}
