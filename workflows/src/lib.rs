//! Orchestrated workflows for the hub task and signal pipeline.
/// PR investigation worktree management.
pub mod fetch;
/// Whether a refresh came back with anything worth caching, for every cache writer.
pub mod freshness;
/// GCP Logging query and entry parsing.
pub mod gcp;
pub(crate) mod git;
/// Loki log query and entry parsing.
pub mod loki;
/// Which pull requests a refresh found waiting on me.
pub mod pr_turns;
/// Asking every source at once, each within a time limit.
pub(crate) mod sources;
/// Unified status fetch across all configured signal sources.
pub mod status;

/// Private feature workflows (optional integrations).
#[cfg(feature = "private")]
pub mod private;
