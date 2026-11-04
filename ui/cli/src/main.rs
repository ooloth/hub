//! Hub CLI: a stub with no subcommands, reserved as the agent-facing toolkit of decision 010.
use clap::Parser;

/// Hub CLI binary.
#[derive(Parser)]
#[command(
    version,
    about = "Hub CLI: no subcommands yet; it is reserved as the agents' toolkit"
)]
struct Cli {}

fn main() {
    let _ = Cli::parse();
}
