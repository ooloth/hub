use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[cfg(feature = "private")]
mod private;
#[derive(Parser)]
#[command(version, about = "Personal command center")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Fetch,
    /// Implement a GitHub issue autonomously, or all status:ready-for-agent issues.
    Implement(commands::implement::ImplementArgs),
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = config::Config::load()?;
    match cli.command {
        Commands::Fetch => commands::fetch::run(&config).await?,
        Commands::Implement(args) => commands::implement::run(&args, &config).await?,
        Commands::Status => commands::status::run(&config).await?,
    }
    Ok(())
}
