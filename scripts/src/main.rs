//! Hub's own dev and ops tooling. See docs/decisions/025-hub-tooling-is-rust-in-the-scripts-crate.md
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// The terms banned from hub, and finding them in text.
mod banned_terms;
mod gh_post_guard;
/// Clippy in every configuration the checkout can build.
mod lint_configurations;
/// Where the checkout this binary was built from lives.
mod repo_root;
#[cfg(test)]
mod repo_state;
/// Text gathered for scanning, and where it came from.
mod scanned_text;
/// Linking hub-private into this checkout.
mod setup_private;
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
    /// Clippy with `-D warnings` in every configuration this checkout can build: always
    /// without hub-private, and with each hub-private module whose sources are linked in.
    ///
    /// Runs every configuration even after one fails, then names the failures.
    LintConfigurations,
    /// Link hub-private's sources and a device's config into this checkout.
    ///
    /// Safe to rerun: an existing link is left alone, a link pointing at another checkout is
    /// reported and left alone, and anything that is not a symlink stops the setup untouched.
    SetupPrivate {
        /// The device, matching hub-private/devices/<device>.toml. Omit it to list them.
        device: Option<String>,
        /// The hub-private checkout, relative to this one.
        #[arg(default_value = "../hub-private")]
        hub_private: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::GuardGhPosts { banned_terms } => gh_post_guard::run(&banned_terms),
        Command::CheckStagedTerms => staged_terms::run(),
        Command::LintConfigurations => lint_configurations::run(),
        Command::SetupPrivate {
            device,
            hub_private,
        } => setup_private::run(device.as_deref(), &hub_private),
    }
}
