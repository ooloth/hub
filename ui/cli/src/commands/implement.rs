use anyhow::{Context, Result};
use clap::Args;
use config::Config;

#[derive(Args)]
pub(crate) struct ImplementArgs {
    /// GitHub repository in owner/name format (e.g. ooloth/hub). Required without --all.
    #[arg(long)]
    repo: Option<String>,

    /// GitHub issue number to implement. Required without --all.
    #[arg(long)]
    issue: Option<u64>,

    /// Find and implement all status:ready-for-agent issues across opted-in repos.
    #[arg(long)]
    all: bool,
}

#[derive(Debug)]
enum Target {
    All,
    One {
        name: String,
        repo: String,
        issue: u64,
    },
}

fn resolve_target(args: &ImplementArgs, config: &Config) -> Result<Target> {
    if args.all {
        return Ok(Target::All);
    }
    let repo = args
        .repo
        .as_deref()
        .context("--repo is required when --all is not set")?;
    let issue = args
        .issue
        .context("--issue is required when --all is not set")?;
    let name = config
        .implement_repos()
        .into_iter()
        .find(|(_, r)| r == repo)
        .map(|(n, _)| n)
        .with_context(|| format!("{repo} is not opted into implement-issue in hub.toml"))?;
    Ok(Target::One {
        name,
        repo: repo.to_string(),
        issue,
    })
}

pub(crate) async fn run(args: &ImplementArgs, config: &Config) -> Result<()> {
    match resolve_target(args, config)? {
        Target::All => workflows::implement::run_all(&config.implement_repos()).await,
        Target::One { name, repo, issue } => {
            workflows::implement::run_one(&name, &repo, issue).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::toml::{Project, WorkflowConfig};

    fn make_config(projects: Vec<Project>) -> Config {
        Config {
            github_token: "tok".into(),
            github_username: "user".into(),
            linear_token: None,
            loki_token: None,
            projects,
            monitor: None,
        }
    }

    fn make_project(name: &str, repo: &str, workflows: Vec<WorkflowConfig>) -> Project {
        Project {
            name: name.into(),
            repo: repo.into(),
            workflow: workflows,
            environment: vec![],
        }
    }

    fn args(repo: Option<&str>, issue: Option<u64>, all: bool) -> ImplementArgs {
        ImplementArgs {
            repo: repo.map(Into::into),
            issue,
            all,
        }
    }

    #[test]
    fn all_flag_resolves_to_all() {
        let config = make_config(vec![]);
        assert!(matches!(
            resolve_target(&args(None, None, true), &config).unwrap(),
            Target::All
        ));
    }

    #[test]
    fn repo_and_issue_resolve_to_one() {
        let config = make_config(vec![make_project(
            "hub",
            "ooloth/hub",
            vec![WorkflowConfig::ImplementIssue],
        )]);
        let target = resolve_target(&args(Some("ooloth/hub"), Some(42), false), &config).unwrap();
        let Target::One { name, repo, issue } = target else {
            panic!("expected One");
        };
        assert_eq!(name, "hub");
        assert_eq!(repo, "ooloth/hub");
        assert_eq!(issue, 42);
    }

    #[test]
    fn missing_repo_errors() {
        let config = make_config(vec![]);
        let err = resolve_target(&args(None, Some(42), false), &config).unwrap_err();
        assert!(err.to_string().contains("--repo is required"));
    }

    #[test]
    fn missing_issue_errors() {
        let config = make_config(vec![]);
        let err = resolve_target(&args(Some("ooloth/hub"), None, false), &config).unwrap_err();
        assert!(err.to_string().contains("--issue is required"));
    }

    #[test]
    fn repo_not_in_config_errors() {
        let config = make_config(vec![make_project(
            "hub",
            "ooloth/hub",
            vec![WorkflowConfig::GithubPrs {
                exclude_authors: vec![],
            }],
        )]);
        let err = resolve_target(&args(Some("ooloth/hub"), Some(1), false), &config).unwrap_err();
        assert!(err.to_string().contains("not opted into implement-issue"));
    }
}
