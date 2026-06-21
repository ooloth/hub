//! Hub CLI — the agent's in-session toolkit for reporting status back to hub.
use clap::Parser;

/// Hub CLI binary.
#[derive(Parser)]
#[command(version, about = "Hub CLI — the agent's in-session toolkit")]
struct Cli {}

fn main() {
    let _ = Cli::parse();
}
