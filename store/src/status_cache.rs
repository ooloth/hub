use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct CachedStatus {
    pub refreshed_at: DateTime<Utc>,
    pub schema_version: i32,
    pub payload: String,
}

/// # Errors
/// Returns an error if the database directory cannot be created or the database file cannot be opened.
pub fn connect() -> Result<Connection> {
    let path = db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create db directory: {}", parent.display()))?;
    }
    maybe_migrate(&legacy_db_path(), &path)?;
    let conn = Connection::open(&path)
        .with_context(|| format!("failed to open db at {}", path.display()))?;
    apply_pragmas(&conn).context("failed to configure connection pragmas")?;
    Ok(conn)
}

fn db_path() -> Result<PathBuf> {
    dirs::home_dir()
        .context("failed to resolve home directory")
        .map(|h| h.join(".hub").join("hub.db"))
}

fn legacy_db_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hub")
        .join("hub.db")
}

/// Copies `legacy` to `new` via VACUUM INTO (WAL-safe) on first run after the path move.
/// No-op when legacy is absent or new already exists.
fn maybe_migrate(legacy: &Path, new: &Path) -> Result<()> {
    if !legacy.exists() || new.exists() {
        return Ok(());
    }
    let src = Connection::open(legacy)
        .with_context(|| format!("failed to open legacy db at {}", legacy.display()))?;
    // VACUUM INTO requires a literal filename; new comes from db_path(), not user input.
    let dest = new.to_str().context("db path is not valid UTF-8")?;
    src.execute_batch(&format!("VACUUM INTO '{dest}'"))
        .with_context(|| format!("failed to migrate db to {}", new.display()))?;
    Ok(())
}

fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("failed to set journal_mode=WAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)
        .context("failed to set busy_timeout")?;
    Ok(())
}

/// # Errors
/// Returns an error if the SQL statement fails.
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

/// # Errors
/// Returns an error if the SQL statement fails.
pub fn upsert(conn: &Connection, payload: &str, schema_version: i32) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let _ = conn
        .execute(
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

/// Returns the cached status only when its `refreshed_at` is within `max_age`
/// of now. `None` means either no row exists or it is older than `max_age` —
/// the caller treats both the same: fetch instead of reusing.
///
/// # Errors
/// Returns an error if the underlying SQL query fails.
pub fn read_if_fresh(conn: &Connection, max_age: chrono::Duration) -> Result<Option<CachedStatus>> {
    match read(conn)? {
        Some(cached) if (Utc::now() - cached.refreshed_at) < max_age => Ok(Some(cached)),
        _ => Ok(None),
    }
}

/// # Errors
/// Returns an error if the SQL query fails or a row value cannot be parsed.
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

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

    #[rstest::rstest]
    #[case::empty_table(None, None)]
    #[case::fresh_row(Some(chrono::Duration::seconds(60)), Some(()))]
    #[case::stale_row(Some(chrono::Duration::seconds(60 * 60)), None)]
    fn read_if_fresh_returns_some_only_when_row_is_within_max_age(
        #[case] row_age: Option<chrono::Duration>,
        #[case] expected_some: Option<()>,
    ) {
        let conn = in_memory();
        if let Some(age) = row_age {
            let written_at = (chrono::Utc::now() - age).to_rfc3339();
            let _ = conn
                .execute(
                    "INSERT INTO status_cache (id, schema_version, refreshed_at, payload)
                 VALUES (1, ?1, ?2, ?3)",
                    rusqlite::params![1i32, written_at, r#"{"items":[]}"#],
                )
                .unwrap();
        }
        let max_age = chrono::Duration::minutes(30);
        let result = read_if_fresh(&conn, max_age).unwrap();
        assert_eq!(result.is_some(), expected_some.is_some());
    }

    #[test]
    fn apply_pragmas_enables_wal_on_file_backed_connection() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("wal.db");
        let conn = Connection::open(&path).unwrap();
        apply_pragmas(&conn).unwrap();
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn apply_pragmas_sets_busy_timeout_to_5000ms() {
        let conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        let timeout: i32 = conn
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .unwrap();
        assert_eq!(timeout, 5000);
    }

    #[test]
    fn second_writer_waits_for_open_write_transaction_instead_of_failing_immediately() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("contended.db");

        let conn1 = Connection::open(&path).unwrap();
        apply_pragmas(&conn1).unwrap();
        ensure_table(&conn1).unwrap();

        let _ = conn1.execute("BEGIN IMMEDIATE", []).unwrap();

        let path_for_writer2 = path.clone();
        let writer2_finished = Arc::new(AtomicBool::new(false));
        let writer2_finished_flag = writer2_finished.clone();

        let writer2 = thread::spawn(move || {
            let conn2 = Connection::open(&path_for_writer2).unwrap();
            apply_pragmas(&conn2).unwrap();
            upsert(&conn2, r#"{"items":[]}"#, 1).unwrap();
            writer2_finished_flag.store(true, Ordering::SeqCst);
        });

        thread::sleep(Duration::from_millis(200));
        assert!(
            !writer2_finished.load(Ordering::SeqCst),
            "writer2 must be blocked on conn1's transaction"
        );

        let _ = conn1.execute("COMMIT", []).unwrap();
        writer2
            .join()
            .expect("writer2 must not panic with SQLITE_BUSY");
        assert!(writer2_finished.load(Ordering::SeqCst));
    }

    #[test]
    fn db_path_ends_with_dot_hub_hub_db() {
        let path = db_path().unwrap();
        assert!(
            path.ends_with(".hub/hub.db"),
            "expected path ending in .hub/hub.db, got {path:?}"
        );
    }

    #[test]
    fn maybe_migrate_copies_data_from_legacy_to_new_path() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("legacy.db");
        let new = tmp.path().join("new.db");

        // Seed legacy with one row
        let src = Connection::open(&legacy).unwrap();
        ensure_table(&src).unwrap();
        upsert(&src, r#"{"items":[42]}"#, 7).unwrap();
        drop(src);

        maybe_migrate(&legacy, &new).unwrap();

        let dst = Connection::open(&new).unwrap();
        let cached = read(&dst).unwrap().unwrap();
        assert_eq!(cached.payload, r#"{"items":[42]}"#);
        assert_eq!(cached.schema_version, 7);
    }

    #[test]
    fn maybe_migrate_is_idempotent_when_new_already_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("legacy.db");
        let new = tmp.path().join("new.db");

        let src = Connection::open(&legacy).unwrap();
        ensure_table(&src).unwrap();
        upsert(&src, r#"{"items":[1]}"#, 1).unwrap();
        drop(src);

        let dst = Connection::open(&new).unwrap();
        ensure_table(&dst).unwrap();
        upsert(&dst, r#"{"items":[99]}"#, 99).unwrap();
        drop(dst);

        // Second call must not overwrite the existing new DB
        maybe_migrate(&legacy, &new).unwrap();

        let dst = Connection::open(&new).unwrap();
        let cached = read(&dst).unwrap().unwrap();
        assert_eq!(
            cached.schema_version, 99,
            "existing new DB must be untouched"
        );
    }

    #[test]
    fn maybe_migrate_is_no_op_when_legacy_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("nonexistent.db");
        let new = tmp.path().join("new.db");

        maybe_migrate(&legacy, &new).unwrap();

        assert!(
            !new.exists(),
            "new DB must not be created when legacy is absent"
        );
    }
}
