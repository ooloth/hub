//! The daemon's health record: one row describing its last pass.
//!
//! It is written in the same transaction as the payload when the pass replaced it (Decision 026).
use anyhow::{Context, Result};
use domain::daemon_pass::Pass;
use rusqlite::{params, Connection, Transaction, TransactionBehavior};

use crate::status_cache::{self, Payload};

/// The shape of the `daemon_health` table.
///
/// Bump it in the change that alters the `CREATE TABLE` below. [`ensure_table`] then drops the
/// old table and creates the new one, which loses at most one pass's record, because the daemon
/// rewrites the row on every pass.
const SCHEMA_VERSION: i32 = 1;

/// Every rule that ties a row's outcome to the rest of it, so a row that contradicts itself cannot
/// be stored, however it is written.
const CREATE_TABLE: &str = "CREATE TABLE daemon_health (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version  INTEGER NOT NULL,
    started_at      TEXT NOT NULL,
    duration_ms     INTEGER NOT NULL CHECK (duration_ms >= 0),
    pid             INTEGER NOT NULL,
    outcome         TEXT NOT NULL
                    CHECK (outcome IN ('ok', 'partial', 'no_source_answered', 'pass_failed')),
    payload_written INTEGER NOT NULL CHECK (payload_written IN (0, 1)),
    items           INTEGER CHECK (items >= 0),
    answered        TEXT NOT NULL CHECK (json_valid(answered) AND json_type(answered) = 'array'),
    failed          TEXT NOT NULL CHECK (json_valid(failed) AND json_type(failed) = 'array'),
    error           TEXT,
    CHECK ((payload_written = 1) = (items IS NOT NULL)),
    CHECK ((outcome = 'pass_failed') = (error IS NOT NULL)),
    CHECK (outcome <> 'ok'
           OR (payload_written = 1 AND json_array_length(failed) = 0)),
    CHECK (outcome <> 'partial'
           OR (json_array_length(answered) > 0 AND json_array_length(failed) > 0)),
    CHECK (outcome <> 'no_source_answered'
           OR (payload_written = 0 AND json_array_length(answered) = 0
               AND json_array_length(failed) > 0)),
    CHECK (outcome <> 'pass_failed'
           OR (payload_written = 0 AND json_array_length(answered) = 0
               AND json_array_length(failed) = 0))
)";

/// Creates the table if it is absent, and recreates it if it was made for another
/// [`SCHEMA_VERSION`].
///
/// # Errors
/// Returns an error if the table cannot be inspected, dropped or created.
pub fn ensure_table(conn: &Connection) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'daemon_health')",
            [],
            |row| row.get(0),
        )
        .context("failed to look for the daemon health table")?;
    if exists {
        // A table from an older shape can lack the column itself, which reads as a mismatch too.
        let stored = conn
            .query_row(
                "SELECT schema_version FROM daemon_health WHERE id = 1",
                [],
                |row| row.get::<_, i32>(0),
            )
            .ok();
        if stored.is_some_and(|version| version == SCHEMA_VERSION) {
            return Ok(());
        }
        conn.execute_batch("DROP TABLE daemon_health")
            .context("failed to drop an outdated daemon health table")?;
    }
    conn.execute_batch(CREATE_TABLE)
        .context("failed to create the daemon health table")
}

/// Records `pass` as the daemon's last pass, and replaces the cached payload with `payload` in
/// the same transaction when the pass wrote one.
///
/// # Errors
/// Returns an error if either write fails, in which case neither is kept.
///
/// # Panics
/// If `payload` is given for a pass that kept the payload, or missing for one that wrote it.
pub fn record(conn: &Connection, pass: &Pass, payload: Option<&Payload>) -> Result<()> {
    if pass.outcome.payload_written() {
        assert!(
            payload.is_some(),
            "a pass that wrote the payload is recorded with it"
        );
    } else {
        assert!(
            payload.is_none(),
            "a pass that kept the payload is recorded without one"
        );
    }

    // Immediate, so a writer already holding the database is waited on through the busy timeout
    // rather than failing when this transaction would upgrade to a write.
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .context("failed to begin recording the pass")?;
    if let Some(payload) = payload {
        status_cache::upsert(&transaction, &payload.json, payload.schema_version)?;
    }
    upsert(&transaction, pass)?;
    transaction
        .commit()
        .context("failed to commit the recorded pass")
}

fn upsert(conn: &Connection, pass: &Pass) -> Result<()> {
    let outcome = &pass.outcome;
    let (answered, failed) = outcome.sources().map_or_else(
        || (serde_json::json!([]), serde_json::json!([])),
        |sources| {
            let failed: Vec<serde_json::Value> = sources
                .failed()
                .iter()
                .map(|failure| {
                    serde_json::json!({
                        "source": failure.source(),
                        "reason": failure.reason().as_str(),
                    })
                })
                .collect();
            (
                serde_json::json!(sources.answered()),
                serde_json::json!(failed),
            )
        },
    );
    let duration_ms = i64::try_from(pass.duration.as_millis())
        .context("a pass lasted longer than the health record can hold")?;
    let items = outcome
        .items()
        .map(i64::try_from)
        .transpose()
        .context("a pass found more signals than the health record can hold")?;
    let _ = conn
        .execute(
            "INSERT INTO daemon_health (
                id, schema_version, started_at, duration_ms, pid, outcome, payload_written,
                items, answered, failed, error
             ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET
                schema_version  = excluded.schema_version,
                started_at      = excluded.started_at,
                duration_ms     = excluded.duration_ms,
                pid             = excluded.pid,
                outcome         = excluded.outcome,
                payload_written = excluded.payload_written,
                items           = excluded.items,
                answered        = excluded.answered,
                failed          = excluded.failed,
                error           = excluded.error",
            params![
                SCHEMA_VERSION,
                pass.started_at.to_rfc3339(),
                duration_ms,
                pass.pid,
                outcome.status().as_str(),
                outcome.payload_written(),
                items,
                answered.to_string(),
                failed.to_string(),
                outcome.failure().map(|failure| failure.reason().as_str()),
            ],
        )
        .context("failed to write the daemon health record")?;
    Ok(())
}

/// The health row as stored, for tests in other crates to check what a pass recorded.
///
/// Not a reader's API: a reader of health also needs the payload from the same snapshot, which
/// Phase 5 of #327 designs with its first readers.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedPass {
    /// When the pass started, as RFC 3339.
    pub started_at: String,
    /// How long it ran, in milliseconds.
    pub duration_ms: i64,
    /// The daemon process that ran it.
    pub pid: i64,
    /// One of `ok`, `partial`, `no_source_answered` or `pass_failed`.
    pub outcome: String,
    /// Whether the pass replaced the payload.
    pub payload_written: bool,
    /// How many signals the new payload holds, when one was written.
    pub items: Option<i64>,
    /// The sources that answered.
    pub answered: Vec<String>,
    /// The sources that failed, as `(source, reason)`.
    pub failed: Vec<(String, String)>,
    /// Why the pass failed, when it did.
    pub error: Option<String>,
}

/// The recorded pass, if any.
///
/// # Errors
/// Returns an error if the row cannot be read or its JSON columns do not parse.
#[cfg(any(test, feature = "test-support"))]
pub fn recorded(conn: &Connection) -> Result<Option<RecordedPass>> {
    use rusqlite::OptionalExtension;

    let row = conn
        .query_row(
            "SELECT started_at, duration_ms, pid, outcome, payload_written, items, answered,
                    failed, error
             FROM daemon_health WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            },
        )
        .optional()
        .context("failed to read the daemon health record")?;
    let Some((
        started_at,
        duration_ms,
        pid,
        outcome,
        payload_written,
        items,
        answered,
        failed,
        error,
    )) = row
    else {
        return Ok(None);
    };
    let answered: Vec<String> =
        serde_json::from_str(&answered).context("answered is not a list of names")?;
    let failed: Vec<serde_json::Value> =
        serde_json::from_str(&failed).context("failed is not a list")?;
    let failed = failed
        .iter()
        .map(|failure| {
            let field = |name: &str| {
                failure[name]
                    .as_str()
                    .map(str::to_string)
                    .with_context(|| format!("a failure has no {name}"))
            };
            Ok((field("source")?, field("reason")?))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(RecordedPass {
        started_at,
        duration_ms,
        pid,
        outcome,
        payload_written,
        items,
        answered,
        failed,
        error,
    }))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::DateTime;
    use domain::daemon_pass::PassOutcome;
    use domain::known_secrets::KnownSecrets;
    use domain::pass_failure::PassFailure;
    use domain::source_failure::SourceFailure;
    use domain::source_outcomes::SourceOutcomes;
    use rstest::rstest;

    use super::*;

    fn in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        status_cache::ensure_table(&conn).unwrap();
        ensure_table(&conn).unwrap();
        conn
    }

    fn no_secrets() -> KnownSecrets {
        KnownSecrets::new(Vec::new())
    }

    fn sources(answered: &[&str], failed: &[&str]) -> SourceOutcomes {
        SourceOutcomes::new(
            answered.iter().map(|name| (*name).to_string()).collect(),
            failed
                .iter()
                .map(|name| SourceFailure::new(*name, &anyhow::anyhow!("down"), &no_secrets()))
                .collect(),
        )
    }

    fn pass(outcome: PassOutcome) -> Pass {
        Pass {
            started_at: DateTime::from_timestamp(1_760_000_000, 0).unwrap(),
            duration: Duration::from_millis(6412),
            pid: 4242,
            outcome,
        }
    }

    fn payload(json: &str) -> Payload {
        Payload {
            json: json.to_string(),
            schema_version: 18,
        }
    }

    fn recorded_row(conn: &Connection) -> RecordedPass {
        recorded(conn).unwrap().expect("a pass was recorded")
    }

    #[rstest]
    #[case::ok(
        PassOutcome::Wrote { items: 3, sources: sources(&["a", "b"], &[]) },
        "ok", true, Some(3), &["a", "b"], &[], None
    )]
    #[case::partial_written(
        PassOutcome::Wrote { items: 3, sources: sources(&["a"], &["b"]) },
        "partial", true, Some(3), &["a"], &["b"], None
    )]
    #[case::partial_kept(
        PassOutcome::Kept { sources: sources(&["a"], &["b"]) },
        "partial", false, None, &["a"], &["b"], None
    )]
    #[case::no_source_answered(
        PassOutcome::Kept { sources: sources(&[], &["a", "b"]) },
        "no_source_answered", false, None, &[], &["a", "b"], None
    )]
    #[case::pass_failed(
        PassOutcome::Failed { failure: PassFailure::new(&anyhow::anyhow!("locked"), &no_secrets()) },
        "pass_failed", false, None, &[], &[], Some("locked")
    )]
    fn a_recorded_pass_reads_back_as_it_was_recorded(
        #[case] outcome: PassOutcome,
        #[case] status: &str,
        #[case] payload_written: bool,
        #[case] items: Option<i64>,
        #[case] answered: &[&str],
        #[case] failed: &[&str],
        #[case] error: Option<&str>,
    ) {
        let conn = in_memory();
        let written = payload_written.then(|| payload("{}"));

        record(&conn, &pass(outcome), written.as_ref()).unwrap();

        assert_eq!(
            recorded_row(&conn),
            RecordedPass {
                started_at: "2025-10-09T08:53:20+00:00".to_string(),
                duration_ms: 6412,
                pid: 4242,
                outcome: status.to_string(),
                payload_written,
                items,
                answered: answered.iter().map(|name| (*name).to_string()).collect(),
                failed: failed
                    .iter()
                    .map(|name| ((*name).to_string(), "down".to_string()))
                    .collect(),
                error: error.map(str::to_string),
            }
        );
    }

    #[test]
    fn a_pass_that_wrote_the_payload_replaces_the_cache_row() {
        let conn = in_memory();
        let outcome = PassOutcome::Wrote {
            items: 1,
            sources: sources(&["a"], &[]),
        };

        record(&conn, &pass(outcome), Some(&payload(r#"{"items":[1]}"#))).unwrap();

        let cached = status_cache::read(&conn).unwrap().unwrap();
        assert_eq!(cached.payload, r#"{"items":[1]}"#);
        assert_eq!(cached.schema_version, 18);
    }

    #[test]
    fn a_health_write_that_fails_leaves_the_cache_row_as_it_was() {
        let conn = in_memory();
        status_cache::upsert(&conn, "before", 18).unwrap();
        let before = status_cache::read(&conn).unwrap().unwrap();
        conn.execute_batch("DROP TABLE daemon_health").unwrap();
        let outcome = PassOutcome::Wrote {
            items: 1,
            sources: sources(&["a"], &[]),
        };

        let result = record(&conn, &pass(outcome), Some(&payload("after")));

        assert!(result.is_err());
        let after = status_cache::read(&conn).unwrap().unwrap();
        assert_eq!(after.payload, before.payload);
        assert_eq!(after.refreshed_at, before.refreshed_at);
    }

    #[test]
    #[should_panic(expected = "a pass that wrote the payload is recorded with it")]
    fn recording_a_written_payload_without_it_is_a_bug() {
        let conn = in_memory();
        let outcome = PassOutcome::Wrote {
            items: 1,
            sources: sources(&["a"], &[]),
        };

        let _ = record(&conn, &pass(outcome), None);
    }

    #[test]
    #[should_panic(expected = "a pass that kept the payload is recorded without one")]
    fn recording_a_kept_payload_with_one_is_a_bug() {
        let conn = in_memory();
        let outcome = PassOutcome::Kept {
            sources: sources(&[], &["a"]),
        };

        let _ = record(&conn, &pass(outcome), Some(&payload("{}")));
    }

    /// Inserts a row that is valid except for `column = value`.
    fn insert_with(conn: &Connection, column: &str, value: &str) -> rusqlite::Result<usize> {
        let mut row = vec![
            ("id", "1"),
            ("schema_version", "1"),
            ("started_at", "'2025-10-09T08:53:20+00:00'"),
            ("duration_ms", "6412"),
            ("pid", "4242"),
            ("outcome", "'partial'"),
            ("payload_written", "1"),
            ("items", "3"),
            ("answered", r#"'["a"]'"#),
            ("failed", r#"'[{"source":"b","reason":"down"}]'"#),
            ("error", "NULL"),
        ];
        for (name, cell) in &mut row {
            if *name == column {
                *cell = value;
            }
        }
        let columns: Vec<&str> = row.iter().map(|(name, _)| *name).collect();
        let values: Vec<&str> = row.iter().map(|(_, cell)| *cell).collect();
        conn.execute(
            &format!(
                "INSERT INTO daemon_health ({}) VALUES ({})",
                columns.join(", "),
                values.join(", ")
            ),
            [],
        )
    }

    #[test]
    fn the_baseline_row_the_refusals_vary_is_accepted() {
        let conn = in_memory();

        assert!(insert_with(&conn, "id", "1").is_ok());
    }

    #[rstest]
    #[case::a_second_row("id", "2")]
    #[case::an_unknown_outcome("outcome", "'fine'")]
    #[case::ok_with_a_failure("outcome", "'ok'")]
    #[case::partial_with_nothing_answered("answered", "'[]'")]
    #[case::no_source_answered_with_a_source_answered("outcome", "'no_source_answered'")]
    #[case::pass_failed_without_an_error("outcome", "'pass_failed'")]
    #[case::an_error_on_a_pass_that_did_not_fail("error", "'locked'")]
    #[case::items_without_a_written_payload("payload_written", "0")]
    #[case::a_written_payload_without_items("items", "NULL")]
    #[case::answered_that_is_not_a_list("answered", r#"'{"a":1}'"#)]
    #[case::failed_that_is_not_json("failed", "'down'")]
    #[case::a_negative_duration("duration_ms", "-1")]
    fn a_row_that_contradicts_itself_is_refused(#[case] column: &str, #[case] value: &str) {
        let conn = in_memory();

        let result = insert_with(&conn, column, value);

        assert!(result.is_err(), "{column} = {value} was accepted");
    }

    #[test]
    fn a_table_from_another_schema_version_is_recreated_empty() {
        let conn = in_memory();
        record(
            &conn,
            &pass(PassOutcome::Kept {
                sources: sources(&[], &["a"]),
            }),
            None,
        )
        .unwrap();
        conn.execute_batch("UPDATE daemon_health SET schema_version = 0")
            .unwrap();

        ensure_table(&conn).unwrap();

        assert_eq!(recorded(&conn).unwrap(), None);
    }

    #[test]
    fn a_table_from_before_schema_versions_is_recreated() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE daemon_health (id INTEGER PRIMARY KEY, outcome TEXT)")
            .unwrap();

        ensure_table(&conn).unwrap();

        assert!(insert_with(&conn, "id", "1").is_ok());
    }

    #[test]
    fn a_table_of_this_schema_version_keeps_its_row() {
        let conn = in_memory();
        record(
            &conn,
            &pass(PassOutcome::Kept {
                sources: sources(&[], &["a"]),
            }),
            None,
        )
        .unwrap();

        ensure_table(&conn).unwrap();

        assert!(recorded(&conn).unwrap().is_some());
    }
}
