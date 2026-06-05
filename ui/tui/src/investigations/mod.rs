//! Investigation sessions: the `i`-key human-triggered workflow in the TUI.
//!
//! This module handles **investigation sessions** — short-lived, interactive Claude Code
//! sessions launched from TUI signal items (PRs, CI failures, Loki alerts, GitHub issues).
//! The human presses `i` on a signal item; a `tmux split-window -h` opens in the current
//! pane so the human and agent share focus immediately.
//!
//! # Not the task dispatch path
//!
//! Investigation sessions are **not** the same as agent task dispatch. If you are
//! implementing task queue dispatch, do not extend or copy this module. The two
//! patterns differ fundamentally:
//!
//! | | Investigation (`i` key) | Task dispatch (queue) |
//! |---|---|---|
//! | Triggered by | Human key press on a signal item | TUI 30s dispatch tick, automatically |
//! | tmux command | `split-window -h` (attached, in-pane) | `new-window -d -n TASK-XXXX` (detached, named) |
//! | Session lifetime | Ephemeral — human-controlled | Persistent until task reaches `in-review` |
//! | Worktree | Ephemeral or PR-specific (cleaned on exit) | Persistent on `agent/TASK-XXXX` branch |
//! | Completion signal | Human closes the pane | `hub task report` CLI call (primary) |
//! | Session ID | Not stored | Stored in `tasks.session_id` (uuid5, injected at dispatch) |
//!
//! Task dispatch code lives in `workflows/src/tasks.rs` (the `dispatch()` function)
//! and is driven by a 30s tick in `ui/tui/src/main.rs`. See
//! `docs/architecture/task-dispatch.md` for the full design.

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
