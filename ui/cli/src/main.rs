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
            println!("github prs   {}", report.github_prs);
        }
    }
    Ok(())
}
