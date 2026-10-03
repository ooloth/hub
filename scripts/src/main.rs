//! Hub's own dev and ops tooling. See docs/decisions/025-hub-tooling-is-rust-in-the-scripts-crate.md
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// The terms banned from hub, and finding them in text.
mod banned_terms;
mod gh_post_guard;
#[cfg(test)]
mod repo_state;
/// Text gathered for scanning, and where it came from.
mod scanned_text;
/// The commit-time check of staged content.
mod staged_terms;

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
    /// Pre-commit check: refuse a commit whose staged content holds a banned term.
    ///
    /// Reads the staged content of the repository in the current directory, including from a
    /// worktree, and finds the list beside the main checkout. Passes when there is no list.
    CheckStagedTerms,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::GuardGhPosts { banned_terms } => gh_post_guard::run(&banned_terms),
        Command::CheckStagedTerms => staged_terms::run(),
    }
}
