//! Orchestrated workflows for the hub task and signal pipeline.
/// Claude Code session state detection and transition decisions.
pub mod agent_session;
pub(crate) mod claude_trust;
/// Task dispatch: claims ready tasks and spawns agent sessions.
pub mod dispatch;
/// PR investigation worktree management.
pub mod fetch;
/// GCP Logging query and entry parsing.
pub mod gcp;
pub(crate) mod git;
/// Loki log query and entry parsing.
pub mod loki;
/// Unified status fetch across all configured signal sources.
pub mod status;
/// Agent captain's log writing and task fold-back utilities.
pub mod task_fold_back;
/// Task CRUD operations exposed to the hub CLI.
pub mod tasks;

/// Private feature workflows (optional integrations).
#[cfg(feature = "private")]
pub mod private;
