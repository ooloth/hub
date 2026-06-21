//! Investigation sessions: the `i`-key human-triggered workflow in the TUI.
//!
//! This module handles **investigation sessions** — short-lived, interactive Claude Code
//! sessions launched from TUI signal items (PRs, CI failures, Loki alerts, GitHub issues).
//! The human presses `i` on a signal item; a `tmux split-window -h` opens in the current
//! pane so the human and agent share focus immediately.

pub(crate) mod ci;
pub(crate) mod gcp;
pub(crate) mod issue;
pub(crate) mod launch;
pub(crate) mod loki;
pub(crate) mod pr;
// Device-specific: home-laptop only via hub-private symlink; setup-private creates
// a stub on other devices so the file always exists when private is enabled.
#[cfg(feature = "private")]
pub(crate) mod media;

pub(crate) use launch::launch;
pub(crate) use launch::{open_in_lazygit, open_in_octo, LaunchConfig, WorktreeSpec};
