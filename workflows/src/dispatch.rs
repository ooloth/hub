//! Task dispatch: claim the oldest ready task and spawn its agent session.
//!
//! This module is the **only place** that should call `tmux new-window` or
//! construct the `claude` command. If tmux is ever replaced or the agent runner
//! changes, this is the file to update — nothing else in the codebase should
//! need to change for those swaps.
//!
//! # Responsibilities
//!
//! - `dispatch()` — called by the TUI's 30-second tick; atomically claims the
//!   oldest `ready` task and spawns a detached named tmux window running Claude
//!   Code in the task's worktree
//! - Window reaping — schedules `tmux kill-window -t TASK-XXXX` after the
//!   5-minute buffer once a task reaches `in-review`; cancels if status reverts
//!
//! # What this module does NOT own
//!
//! - Session state detection (completion, stall, self-heal) → `agent_session`
//! - Worktree creation and cleanup → `fetch`
//! - Task CRUD → `tasks` and `store::tasks`
//!
//! # Design note
//!
//! The coupling to tmux (~5 lines) and to the `claude` CLI flags (~10 lines) is
//! intentional and accepted. See `docs/decisions/015-accept-tmux-and-claude-code-coupling.md`.
//!
//! # Implementation
//!
//! Not yet built. Tracked in issue #279 (S1).

// Implementation goes here (S1).
