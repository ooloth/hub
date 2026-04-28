use anyhow::Result;
use clap::{Parser, Subcommand};
use config::toml::WorkflowConfig;

#[derive(Parser)]
#[command(version, about = "Personal command center")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Status => {
            let config = config::Config::load()?;

            let pr_repos: Vec<String> = config
                .projects
                .iter()
                .filter(|p| {
                    p.workflow
                        .iter()
                        .any(|w| matches!(w, WorkflowConfig::GithubPrs { .. }))
                })
                .map(|p| p.repo.clone())
                .collect();

            let issue_repos: Vec<String> = config
                .projects
                .iter()
                .filter_map(|p| {
                    p.workflow.iter().find_map(|w| {
                        if let WorkflowConfig::GithubIssues {
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
                .collect();

            let assigned_issue_repos: Vec<String> = config
                .projects
                .iter()
                .filter_map(|p| {
                    p.workflow.iter().find_map(|w| {
                        if let WorkflowConfig::GithubIssues {
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
                .collect();

            let report = workflows::status::run(
                &config.github_token,
                &pr_repos,
                &issue_repos,
                &assigned_issue_repos,
            )
            .await?;

            let prs = &report.github_prs;
            println!("github prs ({})", prs.len());
            for pr in prs {
                println!("  {}  {} (#{})  {}", pr.repo, pr.title, pr.number, pr.url);
            }

            let issues = &report.github_issues;
            println!("github issues ({})", issues.len());
            for issue in issues {
                println!(
                    "  {}  {} (#{})  {}",
                    issue.repo, issue.title, issue.number, issue.url
                );
            }
        }
    }
    Ok(())
}
