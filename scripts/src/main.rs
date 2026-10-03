//! Hub's own dev and ops tooling. See docs/decisions/025-hub-tooling-is-rust-in-the-scripts-crate.md
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod gh_post_guard;

/// Hub's dev and ops tooling.
#[derive(Parser)]
#[command(about = "Hub's dev and ops tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Claude Code `PreToolUse` hook: refuse a `gh` call that would publish a banned term.
    ///
    /// Reads the hook's JSON from stdin. Prints a deny decision to stdout when the call
    /// would publish a term from the list, and nothing otherwise. Allows everything when
    /// the list does not exist, which is every checkout without hub-private beside it.
    GuardGhPosts {
        /// The banned-terms list, one term per line.
        #[arg(long)]
        banned_terms: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::GuardGhPosts { banned_terms } => gh_post_guard::run(&banned_terms),
    }
}
