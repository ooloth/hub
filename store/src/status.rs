use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::path::PathBuf;

pub struct CachedStatus {
    pub refreshed_at: DateTime<Utc>,
    pub schema_version: i32,
    pub payload: String,
}

pub fn connect() -> Result<Connection> {
    let path = std::env::var("HUB_DB_PATH")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("hub")
                .join("hub.db")
        });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create db directory: {}", parent.display()))?;
    }
    Connection::open(&path).with_context(|| format!("failed to open db at {}", path.display()))
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS status_cache (
            id              INTEGER PRIMARY KEY,
            schema_version  INTEGER NOT NULL,
            refreshed_at    TEXT NOT NULL,
            payload         TEXT NOT NULL
        );",
    )
    .context("failed to create status_cache table")
}

pub fn upsert(conn: &Connection, payload: &str, schema_version: i32) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO status_cache (id, schema_version, refreshed_at, payload)
         VALUES (1, ?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET
             schema_version = excluded.schema_version,
             refreshed_at   = excluded.refreshed_at,
             payload        = excluded.payload",
        params![schema_version, now, payload],
    )
    .context("failed to upsert status cache")?;
    Ok(())
}

pub fn read(conn: &Connection) -> Result<Option<CachedStatus>> {
    let mut stmt = conn
        .prepare("SELECT schema_version, refreshed_at, payload FROM status_cache WHERE id = 1")
        .context("failed to prepare status cache read")?;

    let mut rows = stmt.query([]).context("failed to query status cache")?;

    match rows.next().context("failed to read status cache row")? {
        None => Ok(None),
        Some(row) => {
            let schema_version: i32 = row.get(0)?;
            let refreshed_at_str: String = row.get(1)?;
            let payload: String = row.get(2)?;
            let refreshed_at = DateTime::parse_from_rfc3339(&refreshed_at_str)
                .with_context(|| format!("invalid refreshed_at timestamp: {refreshed_at_str}"))?
                .with_timezone(&Utc);
            Ok(Some(CachedStatus {
                refreshed_at,
                schema_version,
                payload,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        conn
    }

    #[test]
    fn read_returns_none_on_empty_table() {
        let conn = in_memory();
        assert!(read(&conn).unwrap().is_none());
    }

    #[test]
    fn upsert_then_read_round_trips_payload_and_version() {
        let conn = in_memory();
        upsert(&conn, r#"{"items":[]}"#, 1).unwrap();
        let cached = read(&conn).unwrap().unwrap();
        assert_eq!(cached.payload, r#"{"items":[]}"#);
        assert_eq!(cached.schema_version, 1);
    }

    #[test]
    fn upsert_overwrites_previous_row() {
        let conn = in_memory();
        upsert(&conn, r#"{"items":[]}"#, 1).unwrap();
        upsert(&conn, r#"{"items":[1]}"#, 2).unwrap();
        let cached = read(&conn).unwrap().unwrap();
        assert_eq!(cached.payload, r#"{"items":[1]}"#);
        assert_eq!(cached.schema_version, 2);
    }
}
