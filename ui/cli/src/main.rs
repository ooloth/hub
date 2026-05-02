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
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = config::Config::load()?;
    match cli.command {
        Commands::Status => commands::status::run(&config).await?,
    }
    Ok(())
}
