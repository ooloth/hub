//! Hub CLI: a stub with no subcommands, reserved as the agent-facing toolkit of decision 010.
use clap::Parser;

/// Hub CLI binary.
#[derive(Parser)]
#[command(version, about = "Hub CLI — the agent's in-session toolkit")]
struct Cli {}

fn main() {
    let _ = Cli::parse();
}
