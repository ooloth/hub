//! Investigation sessions: the `i`-key human-triggered workflow in the TUI.
//!
//! This module handles **investigation sessions** — short-lived, interactive Claude Code
//! sessions launched from TUI signal items (PRs, CI failures, Loki alerts, GitHub issues).
//! The human presses `i` on a signal item and a `tmux new-window` opens, named after that
//! signal by `domain::InvestigationWindow`. The TUI keeps its own window, several
//! investigations run side by side, and each one is reachable from tmux's window list.

pub(crate) mod ci;
pub(crate) mod command;
pub(crate) mod gcp;
pub(crate) mod issue;
pub(crate) mod launch;
pub(crate) mod loki;
pub(crate) mod pr;
// Device-specific: the real module is a hub-private symlink present only where the
// `media` feature is on. Every other `private` build compiles the stub, which keeps
// the same signature. See docs/invariants/hub-builds-with-and-without-each-private-module.md
//
// Keep both paths in `cfg_attr`. rustfmt then formats whichever file exists and skips a
// missing `media.rs`, where a plain `#[cfg] mod media;` makes it fail on every device
// without the symlink.
#[cfg(feature = "private")]
#[cfg_attr(feature = "media", path = "media.rs")]
#[cfg_attr(not(feature = "media"), path = "media_stub.rs")]
pub(crate) mod media;

pub(crate) use launch::launch;
pub(crate) use launch::{
    open_in_lazygit, open_in_octo, LaunchConfig, SupportingData, WorktreeSpec,
};
