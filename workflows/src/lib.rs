//! Orchestrated workflows for the hub task and signal pipeline.
/// PR investigation worktree management.
pub mod fetch;
/// GCP Logging query and entry parsing.
pub mod gcp;
pub(crate) mod git;
/// The credential values a failure reason must never contain.
pub mod known_secrets;
/// Loki log query and entry parsing.
pub mod loki;
/// A source that failed during a refresh, and why, in a form safe to log and store.
pub mod source_failure;
/// Asking every source at once, each within a time limit.
pub(crate) mod sources;
/// Unified status fetch across all configured signal sources.
pub mod status;

/// Private feature workflows (optional integrations).
#[cfg(feature = "private")]
pub mod private;
