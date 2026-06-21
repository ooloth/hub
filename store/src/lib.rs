//! `SQLite` persistence layer for hub — status cache.

pub use rusqlite::Connection;

/// Status cache: single-row store for the serialized TUI payload.
pub mod status_cache;
