//! `SQLite` persistence layer for hub: the status cache and the daemon's health record.

pub use rusqlite::Connection;

/// The daemon's health record: one row describing its last pass.
pub mod daemon_health;
/// Reading the cached signals and the daemon's health record together.
pub mod snapshot;
/// Status cache: single-row store for the serialized TUI payload.
pub mod status_cache;
