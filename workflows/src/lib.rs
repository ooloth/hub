//! Orchestrated workflows for the hub task and signal pipeline.
/// PR investigation worktree management.
pub mod fetch;
/// GCP Logging query and entry parsing.
pub mod gcp;
pub(crate) mod git;
/// Loki log query and entry parsing.
pub mod loki;
/// Unified status fetch across all configured signal sources.
pub mod status;

/// Private feature workflows (optional integrations).
#[cfg(feature = "private")]
pub mod private;
