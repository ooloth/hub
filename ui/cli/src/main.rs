use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(version, about = "Agent toolkit for hub project-conditional logic")]
struct Cli {}

#[tokio::main]
async fn main() -> Result<()> {
    Cli::parse();
    Ok(())
}
