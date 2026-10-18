//! The daemon's health record: one row describing its last pass.
//!
//! It is written in the same transaction as the payload when the pass replaced it (Decision 026).
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use domain::credential_attempt::{AppOpening, CredentialAttempt, Notifying};
use domain::credential_failure::CredentialCause;
use domain::daemon_health::{DaemonHealth, DaemonRecord, LastActivity, Schedule};
use domain::daemon_pass::{Pass, PassStatus};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::status_cache::{self, Payload};

/// The shape of the `daemon_health` table.
///
/// Bump it in the change that alters the `CREATE TABLE` below. [`ensure_table`] then drops the
/// old table and creates the new one, which loses at most one pass's record, because the daemon
/// rewrites the row on every pass.
const SCHEMA_VERSION: i32 = 7;

/// Every rule that ties a row's outcome to the rest of it, so a row that contradicts itself cannot
/// be stored, however it is written.
///
/// The `credentials_` outcomes record an attempt to obtain credentials that failed before any
/// pass could run, so no source was asked and the payload was kept.
const CREATE_TABLE: &str = "CREATE TABLE daemon_health (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version  INTEGER NOT NULL,
    started_at      TEXT NOT NULL,
    duration_ms     INTEGER NOT NULL CHECK (duration_ms >= 0),
    pid             INTEGER NOT NULL,
    interval_ms     INTEGER CHECK (interval_ms IS NULL OR interval_ms > 0),
    outcome         TEXT NOT NULL
                    CHECK (outcome IN ('ok', 'partial', 'no_source_answered', 'pass_failed',
                                       'credentials_unreachable', 'credentials_app_not_running',
                                       'credentials_touch_id_prompt_unanswered',
                                       'credentials_macos_prompt_waiting_for_you',
                                       'credentials_app_running_not_answering',
                                       'credentials_failed')),
    payload_written INTEGER NOT NULL CHECK (payload_written IN (0, 1)),
    items           INTEGER CHECK (items >= 0),
    answered        TEXT NOT NULL CHECK (json_valid(answered) AND json_type(answered) = 'array'),
    failed          TEXT NOT NULL CHECK (json_valid(failed) AND json_type(failed) = 'array'),
    error           TEXT,
    opened_app      TEXT CHECK (opened_app IN ('opened', 'failed')),
    open_error      TEXT,
    notified        TEXT CHECK (notified IN ('sent', 'failed')),
    notify_error    TEXT,
    CHECK ((payload_written = 1) = (items IS NOT NULL)),
    CHECK ((notified IS 'failed') = (notify_error IS NOT NULL)),
    CHECK (notified IS NULL OR outcome GLOB 'credentials_*'),
    CHECK ((opened_app IS 'failed') = (open_error IS NOT NULL)),
    CHECK (opened_app IS NULL OR outcome = 'credentials_app_not_running'),
    CHECK ((outcome = 'pass_failed' OR outcome GLOB 'credentials_*')
           = (error IS NOT NULL)),
    CHECK (outcome <> 'ok'
           OR (payload_written = 1 AND json_array_length(failed) = 0)),
    CHECK (outcome <> 'partial'
           OR (json_array_length(answered) > 0 AND json_array_length(failed) > 0)),
    CHECK (outcome <> 'no_source_answered'
           OR (payload_written = 0 AND json_array_length(answered) = 0
               AND json_array_length(failed) > 0)),
    CHECK ((outcome <> 'pass_failed' AND outcome NOT GLOB 'credentials_*')
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

/// Records `pass` as the daemon's last pass, run on `schedule`, and replaces the cached payload
/// with `payload` in the same transaction when the pass wrote one.
///
/// # Errors
/// Returns an error if either write fails, in which case neither is kept.
///
/// # Panics
/// If `payload` is given for a pass that kept the payload, or missing for one that wrote it.
pub fn record(
    conn: &Connection,
    pass: &Pass,
    schedule: Schedule,
    payload: Option<&Payload>,
) -> Result<()> {
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
    upsert(&transaction, pass, schedule)?;
    transaction
        .commit()
        .context("failed to commit the recorded pass")
}

/// Records `attempt` as the daemon's last pass, by a daemon running on `schedule`, leaving the
/// cached payload as it was.
///
/// # Errors
/// Returns an error if the health record cannot be written.
pub fn record_credential_attempt(
    conn: &Connection,
    attempt: &CredentialAttempt,
    schedule: Schedule,
) -> Result<()> {
    let outcome = match attempt.failure.cause() {
        CredentialCause::Unreachable => "credentials_unreachable",
        CredentialCause::AppNotRunning => "credentials_app_not_running",
        CredentialCause::TouchIdPromptUnanswered => "credentials_touch_id_prompt_unanswered",
        CredentialCause::MacosPromptWaitingForYou => "credentials_macos_prompt_waiting_for_you",
        CredentialCause::AppRunningNotAnswering => "credentials_app_running_not_answering",
        CredentialCause::Other => "credentials_failed",
    };
    let duration_ms = i64::try_from(attempt.duration.as_millis())
        .context("an attempt lasted longer than the health record can hold")?;
    let (opened_app, open_error) = match &attempt.opened_app {
        None => (None, None),
        Some(AppOpening::Opened) => (Some("opened"), None),
        Some(AppOpening::Failed { reason }) => (Some("failed"), Some(reason.as_str())),
    };
    let (notified, notify_error) = match &attempt.notified {
        None => (None, None),
        Some(Notifying::Sent) => (Some("sent"), None),
        Some(Notifying::Failed { reason }) => (Some("failed"), Some(reason.as_str())),
    };
    let _ = conn
        .execute(
            "INSERT INTO daemon_health (
                id, schema_version, started_at, duration_ms, pid, interval_ms, outcome,
                payload_written, items, answered, failed, error, opened_app, open_error, notified,
                notify_error
             ) VALUES (1, ?1, ?2, ?3, ?4, ?11, ?5, 0, NULL, '[]', '[]', ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET
                schema_version  = excluded.schema_version,
                started_at      = excluded.started_at,
                duration_ms     = excluded.duration_ms,
                pid             = excluded.pid,
                interval_ms     = excluded.interval_ms,
                outcome         = excluded.outcome,
                payload_written = excluded.payload_written,
                items           = excluded.items,
                answered        = excluded.answered,
                failed          = excluded.failed,
                error           = excluded.error,
                opened_app      = excluded.opened_app,
                open_error      = excluded.open_error,
                notified        = excluded.notified,
                notify_error    = excluded.notify_error",
            params![
                SCHEMA_VERSION,
                attempt.started_at.to_rfc3339(),
                duration_ms,
                attempt.pid,
                outcome,
                attempt.failure.reason().as_str(),
                opened_app,
                open_error,
                notified,
                notify_error,
                interval_ms(schedule)?,
            ],
        )
        .context("failed to write the daemon health record")?;
    Ok(())
}

fn upsert(conn: &Connection, pass: &Pass, schedule: Schedule) -> Result<()> {
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
                id, schema_version, started_at, duration_ms, pid, interval_ms, outcome,
                payload_written, items, answered, failed, error
             ) VALUES (1, ?1, ?2, ?3, ?4, ?11, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET
                schema_version  = excluded.schema_version,
                started_at      = excluded.started_at,
                duration_ms     = excluded.duration_ms,
                pid             = excluded.pid,
                interval_ms     = excluded.interval_ms,
                outcome         = excluded.outcome,
                payload_written = excluded.payload_written,
                items           = excluded.items,
                answered        = excluded.answered,
                failed          = excluded.failed,
                error           = excluded.error,
                opened_app      = NULL,
                open_error      = NULL,
                notified        = NULL,
                notify_error    = NULL",
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
                interval_ms(schedule)?,
            ],
        )
        .context("failed to write the daemon health record")?;
    Ok(())
}

/// The daemon's last record, read inside the caller's transaction so it matches the payload read
/// beside it. A table of another version is reported and left as it is.
pub(crate) fn last_record(conn: &Connection) -> Result<DaemonRecord> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'daemon_health')",
            [],
            |row| row.get(0),
        )
        .context("failed to look for the daemon health table")?;
    if !exists {
        return Ok(DaemonRecord::NoneRecorded);
    }
    // Read alone first: a table of another version may lack the columns this build selects.
    let version: Option<Option<i32>> = conn
        .query_row(
            "SELECT schema_version FROM daemon_health WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .context("failed to read the daemon health record's version")?;
    match version {
        None => return Ok(DaemonRecord::NoneRecorded),
        Some(Some(version)) if version == SCHEMA_VERSION => {}
        Some(_) => return Ok(DaemonRecord::OtherVersion),
    }
    let (started_at, interval_ms, outcome): (String, Option<i64>, String) = conn
        .query_row(
            "SELECT started_at, interval_ms, outcome FROM daemon_health WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .context("failed to read the daemon health record")?;
    Ok(DaemonRecord::Recorded(DaemonHealth {
        started_at: DateTime::parse_from_rfc3339(&started_at)
            .with_context(|| format!("the daemon's last pass has an invalid start: {started_at}"))?
            .with_timezone(&Utc),
        schedule: schedule(interval_ms)?,
        activity: activity(&outcome)?,
    }))
}

fn schedule(interval_ms: Option<i64>) -> Result<Schedule> {
    let Some(interval_ms) = interval_ms else {
        return Ok(Schedule::Once);
    };
    let interval_ms = u64::try_from(interval_ms)
        .ok()
        .filter(|interval_ms| *interval_ms > 0)
        .with_context(|| format!("the daemon recorded an interval of {interval_ms} ms"))?;
    Ok(Schedule::every(Duration::from_millis(interval_ms)))
}

fn activity(outcome: &str) -> Result<LastActivity> {
    let status = match outcome {
        "ok" => PassStatus::Ok,
        "partial" => PassStatus::Partial,
        "no_source_answered" => PassStatus::NoSourceAnswered,
        "pass_failed" => PassStatus::PassFailed,
        "credentials_unreachable"
        | "credentials_app_not_running"
        | "credentials_touch_id_prompt_unanswered"
        | "credentials_macos_prompt_waiting_for_you"
        | "credentials_app_running_not_answering"
        | "credentials_failed" => return Ok(LastActivity::WaitingForCredentials),
        unknown => bail!("the daemon recorded an outcome this build does not know: {unknown}"),
    };
    Ok(LastActivity::Pass(status))
}

/// How `schedule` is stored: the interval in milliseconds, or NULL for a single pass.
fn interval_ms(schedule: Schedule) -> Result<Option<i64>> {
    match schedule {
        Schedule::Once => Ok(None),
        Schedule::Every(interval) => i64::try_from(interval.as_millis())
            .map(Some)
            .context("an interval is longer than the health record can hold"),
    }
}

/// The health row as stored, for tests in other crates to check what a pass recorded.
///
/// Not a reader's API: a reader of health also needs the payload from the same snapshot, which
/// [`crate::snapshot::read`] gives it.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedPass {
    /// When the pass started, as RFC 3339.
    pub started_at: String,
    /// How long it ran, in milliseconds.
    pub duration_ms: i64,
    /// The daemon process that ran it.
    pub pid: i64,
    /// The interval the daemon ran on, in milliseconds, or None for a single pass.
    pub interval_ms: Option<i64>,
    /// One of `ok`, `partial`, `no_source_answered` or `pass_failed`, or for a failed attempt to
    /// obtain credentials, `credentials_unreachable`, `credentials_app_not_running`,
    /// `credentials_touch_id_prompt_unanswered`, `credentials_macos_prompt_waiting_for_you`,
    /// `credentials_app_running_not_answering` or `credentials_failed`.
    pub outcome: String,
    /// Whether the pass replaced the payload.
    pub payload_written: bool,
    /// How many signals the new payload holds, when one was written.
    pub items: Option<i64>,
    /// The sources that answered.
    pub answered: Vec<String>,
    /// The sources that failed, as `(source, reason)`.
    pub failed: Vec<(String, String)>,
    /// Why the pass, or the attempt to obtain credentials, failed, when it did.
    pub error: Option<String>,
    /// `opened` or `failed` when the daemon opened the 1Password app after this attempt.
    pub opened_app: Option<String>,
    /// Why opening the app failed, when it did.
    pub open_error: Option<String>,
    /// `sent` or `failed` when the daemon posted a notification after this attempt.
    pub notified: Option<String>,
    /// Why posting failed, when it did.
    pub notify_error: Option<String>,
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
            "SELECT started_at, duration_ms, pid, interval_ms, outcome, payload_written, items,
                    answered, failed, error, opened_app, open_error, notified, notify_error
             FROM daemon_health WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, bool>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                ))
            },
        )
        .optional()
        .context("failed to read the daemon health record")?;
    let Some((
        started_at,
        duration_ms,
        pid,
        interval_ms,
        outcome,
        payload_written,
        items,
        answered,
        failed,
        error,
        opened_app,
        open_error,
        notified,
        notify_error,
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
        interval_ms,
        outcome,
        payload_written,
        items,
        answered,
        failed,
        error,
        opened_app,
        open_error,
        notified,
        notify_error,
    }))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::DateTime;
    use domain::credential_failure::CredentialFailure;
    use domain::daemon_pass::PassOutcome;
    use domain::known_secrets::KnownSecrets;
    use domain::pass_failure::PassFailure;
    use domain::screen_at_deadline::ScreenAtDeadline;
    use domain::source_failure::SourceFailure;
    use domain::source_outcomes::SourceOutcomes;
    use rstest::rstest;

    use super::*;

    /// The schedule every test records with, unless it is about the schedule.
    const SCHEDULE: Schedule = Schedule::Every(Duration::from_secs(15 * 60));

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

        record(&conn, &pass(outcome), SCHEDULE, written.as_ref()).unwrap();

        assert_eq!(
            recorded_row(&conn),
            RecordedPass {
                started_at: "2025-10-09T08:53:20+00:00".to_string(),
                duration_ms: 6412,
                pid: 4242,
                interval_ms: Some(900_000),
                outcome: status.to_string(),
                payload_written,
                items,
                answered: answered.iter().map(|name| (*name).to_string()).collect(),
                failed: failed
                    .iter()
                    .map(|name| ((*name).to_string(), "down".to_string()))
                    .collect(),
                error: error.map(str::to_string),
                opened_app: None,
                open_error: None,
                notified: None,
                notify_error: None,
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

        record(
            &conn,
            &pass(outcome),
            SCHEDULE,
            Some(&payload(r#"{"items":[1]}"#)),
        )
        .unwrap();

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

        let result = record(&conn, &pass(outcome), SCHEDULE, Some(&payload("after")));

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

        let _ = record(&conn, &pass(outcome), SCHEDULE, None);
    }

    #[test]
    #[should_panic(expected = "a pass that kept the payload is recorded without one")]
    fn recording_a_kept_payload_with_one_is_a_bug() {
        let conn = in_memory();
        let outcome = PassOutcome::Kept {
            sources: sources(&[], &["a"]),
        };

        let _ = record(&conn, &pass(outcome), SCHEDULE, Some(&payload("{}")));
    }

    /// Inserts a row that is valid except for `column = value`.
    fn insert_with(conn: &Connection, column: &str, value: &str) -> rusqlite::Result<usize> {
        let mut row = vec![
            ("id", "1"),
            ("schema_version", "1"),
            ("started_at", "'2025-10-09T08:53:20+00:00'"),
            ("duration_ms", "6412"),
            ("pid", "4242"),
            ("interval_ms", "900000"),
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
    #[case::a_zero_interval("interval_ms", "0")]
    fn a_row_that_contradicts_itself_is_refused(#[case] column: &str, #[case] value: &str) {
        let conn = in_memory();

        let result = insert_with(&conn, column, value);

        assert!(result.is_err(), "{column} = {value} was accepted");
    }

    fn credential_attempt(cause_text: &str, retry_in: Option<Duration>) -> CredentialAttempt {
        CredentialAttempt {
            started_at: DateTime::from_timestamp(1_760_000_000, 0).unwrap(),
            duration: Duration::from_millis(812),
            pid: 4242,
            attempt: 3,
            failure: CredentialFailure::new(&anyhow::anyhow!("{cause_text}"), &no_secrets()),
            opened_app: None,
            notified: None,
            retry_in,
        }
    }

    #[rstest]
    #[case::unreachable(
        "dial tcp: lookup my.1password.com: no such host",
        "credentials_unreachable"
    )]
    #[case::app_not_running(
        "1Password CLI couldn't connect to the 1Password desktop app.",
        "credentials_app_not_running"
    )]
    #[case::touch_id_prompt_unanswered(
        "error initializing client: authorization timeout",
        "credentials_touch_id_prompt_unanswered"
    )]
    #[case::other("\"Nope\" isn't a vault", "credentials_failed")]
    fn a_recorded_credential_attempt_reads_back_as_it_was_recorded(
        #[case] message: &str,
        #[case] outcome: &str,
    ) {
        let conn = in_memory();

        record_credential_attempt(
            &conn,
            &credential_attempt(message, Some(Duration::from_secs(4))),
            SCHEDULE,
        )
        .unwrap();

        assert_eq!(
            recorded_row(&conn),
            RecordedPass {
                started_at: "2025-10-09T08:53:20+00:00".to_string(),
                duration_ms: 812,
                pid: 4242,
                interval_ms: Some(900_000),
                outcome: outcome.to_string(),
                payload_written: false,
                items: None,
                answered: Vec::new(),
                failed: Vec::new(),
                error: Some(message.to_string()),
                opened_app: None,
                open_error: None,
                notified: None,
                notify_error: None,
            }
        );
    }

    #[rstest]
    #[case::macos_prompt(ScreenAtDeadline::MacosAlert { windows: 1 }, "credentials_macos_prompt_waiting_for_you")]
    #[case::app_running(
        ScreenAtDeadline::AppRunningNoAlert,
        "credentials_app_running_not_answering"
    )]
    #[case::app_not_running(ScreenAtDeadline::AppNotRunning, "credentials_app_not_running")]
    fn a_deadline_reads_back_as_who_was_not_answering(
        #[case] screen: ScreenAtDeadline,
        #[case] outcome: &str,
    ) {
        let conn = in_memory();
        let mut attempt = credential_attempt("unused", None);
        attempt.failure = CredentialFailure::no_answer(
            "op://Scripts/GitHub/token",
            Duration::from_secs(90),
            &screen,
            &no_secrets(),
        );

        record_credential_attempt(&conn, &attempt, SCHEDULE).unwrap();

        let row = recorded_row(&conn);
        assert_eq!(row.outcome, outcome);
        assert_eq!(
            row.error.as_deref(),
            Some(attempt.failure.reason().as_str())
        );
    }

    #[rstest]
    #[case::opened(Some(AppOpening::Opened), Some("opened"), None)]
    #[case::failed(
        Some(AppOpening::failed(&anyhow::anyhow!("LSOpenURLsWithRole() failed"), &no_secrets())),
        Some("failed"),
        Some("LSOpenURLsWithRole() failed")
    )]
    #[case::not_tried(None, None, None)]
    fn opening_the_app_reads_back_as_recorded(
        #[case] opened_app: Option<AppOpening>,
        #[case] stored: Option<&str>,
        #[case] open_error: Option<&str>,
    ) {
        let conn = in_memory();
        let mut attempt = credential_attempt(
            "1Password CLI couldn't connect to the 1Password desktop app.",
            Some(Duration::from_secs(1)),
        );
        attempt.opened_app = opened_app;

        record_credential_attempt(&conn, &attempt, SCHEDULE).unwrap();

        let row = recorded_row(&conn);
        assert_eq!(row.outcome, "credentials_app_not_running");
        assert_eq!(row.opened_app.as_deref(), stored);
        assert_eq!(row.open_error.as_deref(), open_error);
    }

    #[test]
    fn a_pass_after_opening_the_app_clears_what_opening_recorded() {
        let conn = in_memory();
        let mut attempt = credential_attempt(
            "1Password CLI couldn't connect to the 1Password desktop app.",
            Some(Duration::from_secs(1)),
        );
        attempt.opened_app = Some(AppOpening::Opened);
        record_credential_attempt(&conn, &attempt, SCHEDULE).unwrap();

        record(
            &conn,
            &pass(PassOutcome::Kept {
                sources: sources(&[], &["a"]),
            }),
            SCHEDULE,
            None,
        )
        .unwrap();

        assert_eq!(recorded_row(&conn).opened_app, None);
    }

    #[rstest]
    #[case::sent(Some(Notifying::Sent), Some("sent"), None)]
    #[case::failed(
        Some(Notifying::failed(&anyhow::anyhow!("osascript exited 1"), &no_secrets())),
        Some("failed"),
        Some("osascript exited 1")
    )]
    #[case::not_due(None, None, None)]
    fn notifying_reads_back_as_recorded(
        #[case] notified: Option<Notifying>,
        #[case] stored: Option<&str>,
        #[case] notify_error: Option<&str>,
    ) {
        let conn = in_memory();
        let mut attempt = credential_attempt("dial tcp", Some(Duration::from_secs(1)));
        attempt.notified = notified;

        record_credential_attempt(&conn, &attempt, SCHEDULE).unwrap();

        let row = recorded_row(&conn);
        assert_eq!(row.notified.as_deref(), stored);
        assert_eq!(row.notify_error.as_deref(), notify_error);
    }

    #[test]
    fn a_pass_after_notifying_clears_what_notifying_recorded() {
        let conn = in_memory();
        let mut attempt = credential_attempt("dial tcp", Some(Duration::from_secs(1)));
        attempt.notified = Some(Notifying::Sent);
        record_credential_attempt(&conn, &attempt, SCHEDULE).unwrap();

        record(
            &conn,
            &pass(PassOutcome::Kept {
                sources: sources(&[], &["a"]),
            }),
            SCHEDULE,
            None,
        )
        .unwrap();

        assert_eq!(recorded_row(&conn).notified, None);
    }

    #[test]
    fn a_credential_attempt_leaves_the_cache_row_as_it_was() {
        let conn = in_memory();
        status_cache::upsert(&conn, "before", 18).unwrap();

        record_credential_attempt(&conn, &credential_attempt("dial tcp", None), SCHEDULE).unwrap();

        assert_eq!(
            status_cache::read(&conn).unwrap().unwrap().payload,
            "before"
        );
    }

    /// Inserts a credential attempt's row, valid except for each `(column, value)` in `changes`.
    fn insert_credential_row(
        conn: &Connection,
        changes: &[(&str, &str)],
    ) -> rusqlite::Result<usize> {
        let mut row = vec![
            ("id", "1"),
            ("schema_version", "6"),
            ("started_at", "'2025-10-09T08:53:20+00:00'"),
            ("duration_ms", "812"),
            ("pid", "4242"),
            ("outcome", "'credentials_unreachable'"),
            ("payload_written", "0"),
            ("items", "NULL"),
            ("answered", "'[]'"),
            ("failed", "'[]'"),
            ("error", "'dial tcp'"),
            ("opened_app", "NULL"),
            ("open_error", "NULL"),
            ("notified", "NULL"),
            ("notify_error", "NULL"),
        ];
        for (name, cell) in &mut row {
            if let Some((_, value)) = changes.iter().find(|(column, _)| column == name) {
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
    fn the_baseline_credential_row_the_refusals_vary_is_accepted() {
        let conn = in_memory();

        assert!(insert_credential_row(&conn, &[]).is_ok());
    }

    #[rstest]
    #[case::an_unknown_cause(&[("outcome", "'credentials_locked'")])]
    #[case::no_error(&[("error", "NULL")])]
    #[case::a_source_answered(&[("answered", r#"'["a"]'"#)])]
    #[case::a_source_failed(&[("failed", r#"'[{"source":"b","reason":"down"}]'"#)])]
    #[case::a_written_payload(&[("payload_written", "1"), ("items", "3")])]
    #[case::opened_the_app_for_another_cause(&[("opened_app", "'opened'")])]
    #[case::an_open_error_without_a_failed_open(&[("opened_app", "'opened'"), ("open_error", "'x'")])]
    #[case::a_failed_open_without_its_error(&[("outcome", "'credentials_app_not_running'"), ("opened_app", "'failed'")])]
    #[case::a_notify_error_without_a_failed_post(&[("notified", "'sent'"), ("notify_error", "'x'")])]
    #[case::a_failed_post_without_its_error(&[("notified", "'failed'")])]
    #[case::an_unknown_notifying(&[("notified", "'queued'")])]
    #[case::an_unknown_opening(&[("outcome", "'credentials_app_not_running'"), ("opened_app", "'tried'")])]
    fn a_credential_row_that_contradicts_itself_is_refused(#[case] changes: &[(&str, &str)]) {
        let conn = in_memory();

        let result = insert_credential_row(&conn, changes);

        assert!(result.is_err(), "{changes:?} was accepted");
    }

    #[test]
    fn a_table_from_another_schema_version_is_recreated_empty() {
        let conn = in_memory();
        record(
            &conn,
            &pass(PassOutcome::Kept {
                sources: sources(&[], &["a"]),
            }),
            SCHEDULE,
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
            SCHEDULE,
            None,
        )
        .unwrap();

        ensure_table(&conn).unwrap();

        assert!(recorded(&conn).unwrap().is_some());
    }

    fn table_exists(conn: &Connection) -> bool {
        conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'daemon_health')",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn a_table_from_another_version_reads_as_such_and_is_left_unchanged() {
        let conn = Connection::open_in_memory().unwrap();
        status_cache::ensure_table(&conn).unwrap();
        conn.execute_batch(
            "CREATE TABLE daemon_health (id INTEGER PRIMARY KEY, schema_version INTEGER, outcome TEXT);
             INSERT INTO daemon_health VALUES (1, 6, 'ok');",
        )
        .unwrap();

        assert_eq!(last_record(&conn).unwrap(), DaemonRecord::OtherVersion);
        let left: (i32, String) = conn
            .query_row(
                "SELECT schema_version, outcome FROM daemon_health",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(left, (6, "ok".to_string()));
    }

    #[test]
    fn an_absent_table_reads_as_no_record_and_is_not_created() {
        let conn = Connection::open_in_memory().unwrap();
        status_cache::ensure_table(&conn).unwrap();

        assert_eq!(last_record(&conn).unwrap(), DaemonRecord::NoneRecorded);
        assert!(!table_exists(&conn));
    }

    #[rstest]
    #[case::a_zero_interval("interval_ms", "0")]
    #[case::an_unknown_outcome("outcome", "'fine'")]
    fn a_row_this_builds_schema_does_not_allow_is_an_error_to_read(
        #[case] column: &str,
        #[case] value: &str,
    ) {
        let conn = in_memory();
        record(
            &conn,
            &pass(PassOutcome::Kept {
                sources: sources(&["a"], &["b"]),
            }),
            Schedule::every(Duration::from_secs(60)),
            None,
        )
        .unwrap();
        // As a daemon whose CHECK clauses differ from this build's could have written it.
        conn.execute_batch("PRAGMA ignore_check_constraints = ON")
            .unwrap();
        let _ = conn
            .execute(&format!("UPDATE daemon_health SET {column} = {value}"), [])
            .unwrap();

        assert!(last_record(&conn).is_err(), "{column} = {value} was read");
    }
}
