//! `SQLite` persistence layer for hub: the status cache and the daemon's health record.

pub use rusqlite::Connection;

/// The daemon's health record: one row describing its last pass.
pub mod daemon_health;
/// Status cache: single-row store for the serialized TUI payload.
pub mod status_cache;
