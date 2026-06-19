//! `SQLite` persistence layer for hub — status cache and task store.

pub use rusqlite::Connection;

/// Status cache: single-row store for the serialized TUI payload.
pub mod status_cache;
/// Task comment store: append-only comment thread per task.
pub mod task_comments;
/// Task store: lifecycle CRUD and query operations for hub tasks.
pub mod tasks;
