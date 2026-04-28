use anyhow::Result;
use clap::{Parser, Subcommand};

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
            let report = workflows::status::run(&config.github_token).await?;
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
