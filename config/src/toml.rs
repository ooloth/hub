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
    pub service: Option<String>,
    pub cluster: Option<String>,
    pub namespace: Option<String>,
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
    GithubIssues {
        #[serde(default)]
        exclude_labels: Vec<String>,
        #[serde(default)]
        assigned_only: bool,
    },
    #[serde(rename = "user-activity-gcp")]
    UserActivityGcp {
        #[serde(default)]
        include_users: Vec<String>,
        #[serde(default)]
        exclude_users: Vec<String>,
    },
    #[serde(rename = "errors-gcp")]
    ErrorsGcp {
        #[serde(default)]
        exclude_users: Vec<String>,
    },
    #[serde(rename = "warnings-gcp")]
    WarningsGcp {
        #[serde(default)]
        exclude_users: Vec<String>,
    },
    #[serde(rename = "user-activity-loki")]
    UserActivityLoki {
        #[serde(default)]
        include_users: Vec<String>,
        #[serde(default)]
        exclude_users: Vec<String>,
    },
    #[serde(rename = "errors-loki")]
    ErrorsLoki {
        #[serde(default)]
        exclude_users: Vec<String>,
    },
    #[serde(rename = "warnings-loki")]
    WarningsLoki {
        #[serde(default)]
        exclude_users: Vec<String>,
    },
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Monitor {
    #[serde(default)]
    pub workflow: Vec<WorkflowConfig>,
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
    #[case("errors-gcp", WorkflowConfig::ErrorsGcp { exclude_users: vec![] })]
    #[case("errors-loki", WorkflowConfig::ErrorsLoki { exclude_users: vec![] })]
    #[case("github-ci", WorkflowConfig::GithubCi { lookback: None })]
    #[case("github-issues", WorkflowConfig::GithubIssues { exclude_labels: vec![], assigned_only: false })]
    #[case("github-prs", WorkflowConfig::GithubPrs { exclude_authors: vec![] })]
    #[case("user-activity-gcp", WorkflowConfig::UserActivityGcp { include_users: vec![], exclude_users: vec![] })]
    #[case("user-activity-loki", WorkflowConfig::UserActivityLoki { include_users: vec![], exclude_users: vec![] })]
    #[case("warnings-gcp", WorkflowConfig::WarningsGcp { exclude_users: vec![] })]
    #[case("warnings-loki", WorkflowConfig::WarningsLoki { exclude_users: vec![] })]
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
    fn github_issues_with_exclude_labels() {
        let result = parse(
            r#"
            [[project]]
            name = "hub"
            repo = "ooloth/hub"

            [[project.workflow]]
            name = "github-issues"
            exclude_labels = ["wontfix", "duplicate"]
        "#,
        )
        .unwrap();
        assert_eq!(
            result.project[0].workflow,
            vec![WorkflowConfig::GithubIssues {
                exclude_labels: vec!["wontfix".into(), "duplicate".into()],
                assigned_only: false,
            }]
        );
    }

    #[test]
    fn environment_with_gcp_workflow() {
        let result = parse(
            r#"
            [[project]]
            name = "my-app"
            repo = "org/my-app"

            [[project.environment]]
            env = "prod"
            gcp_project = "my-org-prod"
            service = "my-app"

            [[project.environment.workflow]]
            name = "user-activity-gcp"
            exclude_users = ["bot@example.com"]
        "#,
        )
        .unwrap();
        let env = &result.project[0].environment[0];
        assert_eq!(env.env, "prod");
        assert_eq!(env.gcp_project.as_deref(), Some("my-org-prod"));
        assert_eq!(
            env.workflow,
            vec![WorkflowConfig::UserActivityGcp {
                include_users: vec![],
                exclude_users: vec!["bot@example.com".into()],
            }]
        );
    }

    #[test]
    fn monitor_with_workflow() {
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
            vec![WorkflowConfig::GithubPrs {
                exclude_authors: vec![]
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
            cluster = "internal-cluster"
            namespace = "default"

            [[project.environment.workflow]]
            name = "user-activity-loki"
            exclude_users = ["bot@example.com"]

            [[project.environment.workflow]]
            name = "errors-loki"

            [[project.environment.workflow]]
            name = "warnings-loki"

            [[project.environment]]
            env = "prod"
            gcp_project = "org-prod"
            gcp_region = "us-central1"
            service = "myapp"

            [[project.environment.workflow]]
            name = "user-activity-gcp"
            include_users = ["alice@example.com"]

            [[project.environment.workflow]]
            name = "errors-gcp"
            exclude_users = ["bot@example.com"]

            [[project.environment.workflow]]
            name = "warnings-gcp"

            [[monitor.workflow]]
            name = "github-prs"
            exclude_authors = ["dependabot", "renovate"]
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

        // Arbitrary exclude_users lists survive a parse round-trip.
        #[test]
        fn exclude_users_round_trip(
            users in proptest::collection::vec("[a-zA-Z][a-zA-Z0-9@._-]{0,30}", 0..10),
        ) {
            let list = users.iter().map(|u| format!("\"{u}\"")).collect::<Vec<_>>().join(", ");
            let toml = format!(
                "[[project]]\nname = \"hub\"\nrepo = \"ooloth/hub\"\n\
                 [[project.workflow]]\nname = \"errors-gcp\"\nexclude_users = [{list}]\n"
            );
            let result = parse(&toml).unwrap();
            let WorkflowConfig::ErrorsGcp { exclude_users } = &result.project[0].workflow[0] else {
                panic!("expected ErrorsGcp");
            };
            prop_assert_eq!(exclude_users, &users);
        }
    }
}
