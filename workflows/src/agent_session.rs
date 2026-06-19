//! Claude Code session state detection for the task dispatch pipeline.
//!
//! This module is the **only place** that reads `~/.claude/sessions/<pid>.json`.
//! If that file format changes or a different agent runner is used, only this
//! module needs updating — the rest of the dispatch pipeline is unaffected.
//!
//! # Signals used
//!
//! Claude Code writes a JSON state file for every running process:
//!
//! ```json
//! {
//!   "pid":       42899,
//!   "sessionId": "<uuid5 injected at dispatch>",
//!   "cwd":       "<worktree path>",
//!   "status":    "busy" | "idle",
//!   "updatedAt": <epoch-ms, changes on every status transition>
//! }
//! ```
//!
//! Hub infers agent progress from `status` and `updatedAt`:
//!
//! | Session file state | Condition | Task transition |
//! |---|---|---|
//! | present, `status="busy"` | `updatedAt` advancing | stay `in-progress` |
//! | present, `status="busy"` | `updatedAt` stale >15 min | → `blocked` |
//! | present, `status="idle"` | >30s, no `in-review` in DB | → `in-review` (fallback) |
//! | absent | no `in-review` in DB | → `in-review` (crash recovery) |
//! | was `blocked`, `status="busy"` | `updatedAt` advancing | → `in-progress` (self-heal) |
//!
//! See `docs/architecture/task-dispatch.md` "Session file signal reference" for
//! the full table and risk notes.
//!
//! # Finding the session file
//!
//! Hub injects `--session-id <uuid5>` at dispatch and stores the value in
//! `tasks.session_id`. To locate the running process's state file, scan
//! `~/.claude/sessions/*.json` for a record whose `sessionId` field matches.
//! The PID is unknown at dispatch time; the UUID is the stable identifier.
//!
//! # Risk
//!
//! `~/.claude/sessions/` is an **undocumented internal API**. Anthropic could
//! change or remove it without notice. If polling fails silently, tasks stay
//! `in-progress` until manual correction via the `s` submenu. The primary
//! signal (`hub task report` CLI call) is independent and unaffected.
//!
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use domain::{Task, TaskStatus};

/// Idle session must sit idle this long before completion is inferred — guards
/// against the brief idle blip between an agent's turns.
const IDLE_GRACE_SECS: i64 = 30;
/// A dispatched task whose session file is still absent is given this long for
/// Claude to create it before absence is read as a crash.
const CRASH_GRACE_SECS: i64 = 30;
/// A `busy` session whose `updatedAt` has not advanced for this long is treated
/// as stalled. `updatedAt` advances on token/message boundaries, not per tool
/// call, so this is a "no output for N" detector; self-heal recovers false hits.
const STALL_SECS: i64 = 15 * 60;

/// Time windows that turn raw session signals into transitions. Carried as a
/// value (not hardcoded) so tests can exercise the boundaries with small spans.
#[derive(Clone, Copy, Debug)]
pub struct PollThresholds {
    pub idle_grace: Duration,
    pub crash_grace: Duration,
    pub stall: Duration,
}

impl PollThresholds {
    /// The thresholds used in production (see the module constants).
    #[must_use]
    pub fn production() -> Self {
        Self {
            idle_grace: Duration::seconds(IDLE_GRACE_SECS),
            crash_grace: Duration::seconds(CRASH_GRACE_SECS),
            stall: Duration::seconds(STALL_SECS),
        }
    }
}

/// The agent runner's reported activity for a session.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Generating — actively working.
    Busy,
    /// At an idle prompt — the turn finished but the process is still alive.
    Idle,
    /// Any value this version doesn't recognize. Kept as a forward-compatible
    /// fallback for the undocumented session-file format; never auto-transitions.
    #[serde(other)]
    Unknown,
}

/// What a session file says about a running agent: its activity and when that
/// activity last changed. Absence of a file is modeled by `Option<SessionSnapshot>`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionSnapshot {
    pub status: SessionStatus,
    pub updated_at: DateTime<Utc>,
}

/// An automatic task-status transition inferred from a session signal. Named so
/// the *reason* survives into logs and tests, even when two reasons share a target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Transition {
    /// Session went idle past the grace window — the agent finished its turn
    /// without reporting (fallback for a missed `hub task report`).
    Completed,
    /// Session file vanished while in-progress — unclean exit or kill.
    CrashRecovered,
    /// Session is busy but its `updatedAt` went stale — frozen or wedged.
    Stalled,
    /// A blocked task's session resumed work — recover automatically.
    SelfHealed,
}

impl Transition {
    /// The status this transition moves the task to.
    #[must_use]
    pub fn target(self) -> TaskStatus {
        match self {
            Self::Completed | Self::CrashRecovered => TaskStatus::InReview,
            Self::Stalled => TaskStatus::Blocked,
            Self::SelfHealed => TaskStatus::InProgress,
        }
    }
}

/// The shape of a `~/.claude/sessions/<pid>.json` record. Only the fields hub
/// reads are declared; the rest are ignored.
#[derive(Deserialize)]
struct RawSessionFile {
    #[serde(rename = "sessionId")]
    session_id: String,
    status: SessionStatus,
    #[serde(rename = "updatedAt")]
    updated_at_ms: i64,
}

/// Parses one session-file body and returns its snapshot only if it belongs to
/// `want_session_id`. Returns `None` if the JSON is unparseable, the id differs,
/// or the timestamp is out of range. Pure — the unit of session-format coupling.
fn parse_snapshot(json: &str, want_session_id: &str) -> Option<SessionSnapshot> {
    let raw: RawSessionFile = serde_json::from_str(json).ok()?;
    if raw.session_id != want_session_id {
        return None;
    }
    let updated_at = DateTime::from_timestamp_millis(raw.updated_at_ms)?;
    Some(SessionSnapshot {
        status: raw.status,
        updated_at,
    })
}

/// Returns `~/.claude/sessions`, where Claude Code writes one `<pid>.json` per
/// running session.
fn sessions_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".claude").join("sessions")
}

/// Scans `sessions_dir` for the session whose `sessionId` matches and returns its
/// snapshot. `Ok(None)` means no live file matches (absent — a crash candidate);
/// `Err` means the directory itself could not be read (don't infer anything).
///
/// The PID is unknown at dispatch time, so the file name can't be guessed — the
/// injected UUID is the stable key, found by reading each record.
///
/// # Errors
/// Returns an error if the session directory cannot be read or a file operation fails.
pub fn read_session_snapshot(
    sessions_dir: &Path,
    session_id: &str,
) -> Result<Option<SessionSnapshot>> {
    let entries = match std::fs::read_dir(sessions_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e)
                .with_context(|| format!("reading sessions dir {}", sessions_dir.display()))
        }
    };

    for entry in entries {
        let path = entry
            .with_context(|| format!("reading entry in {}", sessions_dir.display()))?
            .path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // A file can vanish or be rewritten mid-scan; skip rather than fail.
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(snapshot) = parse_snapshot(&text, session_id) {
            return Ok(Some(snapshot));
        }
    }
    Ok(None)
}

/// Decides the automatic transition (if any) for a task given its current status,
/// when the DB last touched it, and its session snapshot. Pure: no I/O, no clock.
///
/// Only `in-progress` and `blocked` tasks transition — every other status returns
/// `None`, so a task already reported `in-review` (or terminal) is never disturbed
/// (the "no in-review yet" guard falls out of matching on `current`).
#[must_use]
pub fn decide_transition(
    current: TaskStatus,
    task_updated_at: DateTime<Utc>,
    snapshot: Option<SessionSnapshot>,
    now: DateTime<Utc>,
    thresholds: &PollThresholds,
) -> Option<Transition> {
    match current {
        TaskStatus::InProgress => match snapshot {
            Some(s) => match s.status {
                SessionStatus::Idle if now - s.updated_at > thresholds.idle_grace => {
                    Some(Transition::Completed)
                }
                SessionStatus::Busy if now - s.updated_at > thresholds.stall => {
                    Some(Transition::Stalled)
                }
                _ => None,
            },
            None if now - task_updated_at > thresholds.crash_grace => {
                Some(Transition::CrashRecovered)
            }
            None => None,
        },
        TaskStatus::Blocked => match snapshot {
            Some(s)
                if s.status == SessionStatus::Busy && now - s.updated_at <= thresholds.stall =>
            {
                Some(Transition::SelfHealed)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Polls every in-progress/blocked task's session file, applies the inferred
/// transition, and returns the refreshed task list for the TUI to patch in.
///
/// Errors reading a single task's session (unreadable sessions dir, unparseable
/// `updated_at`) are logged and skipped — one bad task never blocks the others or
/// the refresh. `now` is injected so the decision stays deterministic and testable.
///
/// # Errors
/// Returns an error if the file cannot be read or written.
pub fn poll_sessions(now: DateTime<Utc>) -> Result<Vec<Task>> {
    let conn = store::status_cache::connect()?;
    store::tasks::ensure_table(&conn)?;
    poll_sessions_inner(&conn, &sessions_dir(), now, &PollThresholds::production())
}

/// The testable core of [`poll_sessions`]: applies transitions over `conn` using
/// session files in `sessions_dir`, then returns the refreshed list. Split out so
/// it can run against an in-memory DB and a temp dir without touching `~`.
fn poll_sessions_inner(
    conn: &store::Connection,
    sessions_dir: &Path,
    now: DateTime<Utc>,
    thresholds: &PollThresholds,
) -> Result<Vec<Task>> {
    let tasks = store::tasks::list_visible(conn)?;

    for task in &tasks {
        if !matches!(task.status, TaskStatus::InProgress | TaskStatus::Blocked) {
            continue;
        }
        let Some(session_id) = &task.session_id else {
            continue;
        };
        let snapshot = match read_session_snapshot(sessions_dir, session_id) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                eprintln!("warn: session poll skipped {}: {e:#}", task.id);
                continue;
            }
        };
        let task_updated_at = match DateTime::parse_from_rfc3339(&task.updated_at) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(e) => {
                eprintln!(
                    "warn: session poll skipped {} — unparseable updated_at {:?}: {e}",
                    task.id, task.updated_at
                );
                continue;
            }
        };
        if let Some(transition) =
            decide_transition(task.status, task_updated_at, snapshot, now, thresholds)
        {
            store::tasks::update_status(conn, &task.id, transition.target())
                .with_context(|| format!("applying {transition:?} to {}", task.id))?;
        }
    }

    store::tasks::list_visible(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(status: SessionStatus, secs_ago: i64, now: DateTime<Utc>) -> Option<SessionSnapshot> {
        Some(SessionSnapshot {
            status,
            updated_at: now - Duration::seconds(secs_ago),
        })
    }

    fn thresholds() -> PollThresholds {
        PollThresholds::production()
    }

    // ── parse_snapshot ────────────────────────────────────────────────────────

    #[test]
    fn parse_snapshot_reads_busy_status_and_timestamp() {
        let json = r#"{"pid":42,"sessionId":"abc","status":"busy","updatedAt":1780716831279}"#;
        let s = parse_snapshot(json, "abc").expect("should parse matching record");
        assert_eq!(s.status, SessionStatus::Busy);
        assert_eq!(
            s.updated_at,
            DateTime::from_timestamp_millis(1780716831279).unwrap()
        );
    }

    #[test]
    fn parse_snapshot_reads_idle_status() {
        let json = r#"{"sessionId":"abc","status":"idle","updatedAt":1780716831279}"#;
        assert_eq!(
            parse_snapshot(json, "abc").unwrap().status,
            SessionStatus::Idle
        );
    }

    #[test]
    fn parse_snapshot_maps_unrecognized_status_to_unknown() {
        let json = r#"{"sessionId":"abc","status":"compacting","updatedAt":1780716831279}"#;
        assert_eq!(
            parse_snapshot(json, "abc").unwrap().status,
            SessionStatus::Unknown
        );
    }

    #[test]
    fn parse_snapshot_returns_none_for_non_matching_session_id() {
        let json = r#"{"sessionId":"other","status":"busy","updatedAt":1780716831279}"#;
        assert_eq!(parse_snapshot(json, "abc"), None);
    }

    #[test]
    fn parse_snapshot_returns_none_for_malformed_json() {
        assert_eq!(parse_snapshot("{not json", "abc"), None);
    }

    // ── decide_transition: in-progress ────────────────────────────────────────

    #[test]
    fn idle_past_grace_completes() {
        let now = Utc::now();
        let t = decide_transition(
            TaskStatus::InProgress,
            now,
            snap(SessionStatus::Idle, 31, now),
            now,
            &thresholds(),
        );
        assert_eq!(t, Some(Transition::Completed));
    }

    #[test]
    fn idle_within_grace_waits() {
        let now = Utc::now();
        let t = decide_transition(
            TaskStatus::InProgress,
            now,
            snap(SessionStatus::Idle, 29, now),
            now,
            &thresholds(),
        );
        assert_eq!(t, None);
    }

    #[test]
    fn idle_exactly_at_grace_waits() {
        let now = Utc::now();
        let t = decide_transition(
            TaskStatus::InProgress,
            now,
            snap(SessionStatus::Idle, 30, now),
            now,
            &thresholds(),
        );
        assert_eq!(t, None, "boundary is strictly greater-than");
    }

    #[test]
    fn busy_stale_past_stall_blocks() {
        let now = Utc::now();
        let t = decide_transition(
            TaskStatus::InProgress,
            now,
            snap(SessionStatus::Busy, 15 * 60 + 1, now),
            now,
            &thresholds(),
        );
        assert_eq!(t, Some(Transition::Stalled));
    }

    #[test]
    fn busy_fresh_stays_in_progress() {
        let now = Utc::now();
        let t = decide_transition(
            TaskStatus::InProgress,
            now,
            snap(SessionStatus::Busy, 60, now),
            now,
            &thresholds(),
        );
        assert_eq!(t, None);
    }

    #[test]
    fn absent_past_crash_grace_recovers() {
        let now = Utc::now();
        let dispatched = now - Duration::seconds(31);
        let t = decide_transition(TaskStatus::InProgress, dispatched, None, now, &thresholds());
        assert_eq!(t, Some(Transition::CrashRecovered));
    }

    #[test]
    fn absent_within_crash_grace_waits_for_file() {
        let now = Utc::now();
        let dispatched = now - Duration::seconds(29);
        let t = decide_transition(TaskStatus::InProgress, dispatched, None, now, &thresholds());
        assert_eq!(
            t, None,
            "freshly dispatched: Claude has not written the file yet"
        );
    }

    #[test]
    fn in_progress_unknown_status_waits() {
        let now = Utc::now();
        let t = decide_transition(
            TaskStatus::InProgress,
            now,
            snap(SessionStatus::Unknown, 999, now),
            now,
            &thresholds(),
        );
        assert_eq!(t, None);
    }

    // ── decide_transition: blocked ────────────────────────────────────────────

    #[test]
    fn blocked_busy_fresh_self_heals() {
        let now = Utc::now();
        let t = decide_transition(
            TaskStatus::Blocked,
            now,
            snap(SessionStatus::Busy, 60, now),
            now,
            &thresholds(),
        );
        assert_eq!(t, Some(Transition::SelfHealed));
    }

    #[test]
    fn blocked_busy_still_stale_stays_blocked() {
        let now = Utc::now();
        let t = decide_transition(
            TaskStatus::Blocked,
            now,
            snap(SessionStatus::Busy, 15 * 60 + 1, now),
            now,
            &thresholds(),
        );
        assert_eq!(t, None);
    }

    #[test]
    fn blocked_idle_stays_blocked() {
        let now = Utc::now();
        let t = decide_transition(
            TaskStatus::Blocked,
            now,
            snap(SessionStatus::Idle, 999, now),
            now,
            &thresholds(),
        );
        assert_eq!(
            t, None,
            "blocked is a fine resting state; only busy self-heals"
        );
    }

    #[test]
    fn blocked_absent_stays_blocked() {
        let now = Utc::now();
        let t = decide_transition(
            TaskStatus::Blocked,
            now - Duration::seconds(999),
            None,
            now,
            &thresholds(),
        );
        assert_eq!(t, None, "a crash while blocked is not a completion");
    }

    // ── decide_transition: untouched statuses ─────────────────────────────────

    #[test]
    fn non_active_status_never_transitions() {
        let now = Utc::now();
        for status in [
            TaskStatus::InReview,
            TaskStatus::Backlog,
            TaskStatus::Ready,
            TaskStatus::Done,
            TaskStatus::Failed,
            TaskStatus::Cancelled,
        ] {
            let t = decide_transition(
                status,
                now,
                snap(SessionStatus::Idle, 999, now),
                now,
                &thresholds(),
            );
            assert_eq!(t, None, "{status:?} must not auto-transition");
        }
    }

    // ── Transition::target ────────────────────────────────────────────────────

    #[test]
    fn transition_target_maps_each_variant() {
        assert_eq!(Transition::Completed.target(), TaskStatus::InReview);
        assert_eq!(Transition::CrashRecovered.target(), TaskStatus::InReview);
        assert_eq!(Transition::Stalled.target(), TaskStatus::Blocked);
        assert_eq!(Transition::SelfHealed.target(), TaskStatus::InProgress);
    }

    // ── read_session_snapshot (integration over a temp dir) ───────────────────

    #[test]
    fn read_session_snapshot_finds_matching_record_among_several() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("100.json"),
            r#"{"sessionId":"other","status":"busy","updatedAt":1780716831279}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("200.json"),
            r#"{"sessionId":"target","status":"idle","updatedAt":1780716831279}"#,
        )
        .unwrap();

        let found = read_session_snapshot(dir.path(), "target").unwrap();
        assert_eq!(found.unwrap().status, SessionStatus::Idle);
    }

    #[test]
    fn read_session_snapshot_returns_none_when_no_record_matches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("100.json"),
            r#"{"sessionId":"other","status":"busy","updatedAt":1780716831279}"#,
        )
        .unwrap();
        assert_eq!(read_session_snapshot(dir.path(), "target").unwrap(), None);
    }

    #[test]
    fn read_session_snapshot_skips_malformed_file_and_finds_valid_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.json"), "{ not json").unwrap();
        std::fs::write(
            dir.path().join("good.json"),
            r#"{"sessionId":"target","status":"busy","updatedAt":1780716831279}"#,
        )
        .unwrap();
        let found = read_session_snapshot(dir.path(), "target").unwrap();
        assert_eq!(found.unwrap().status, SessionStatus::Busy);
    }

    #[test]
    fn read_session_snapshot_returns_none_when_dir_absent() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert_eq!(read_session_snapshot(&missing, "target").unwrap(), None);
    }

    // ── poll_sessions_inner (integration over real SQLite + temp dir) ─────────

    use domain::TaskKind;
    use store::Connection;

    /// Creates an in-progress task with a session id and returns (id, session_id).
    fn dispatched_task(conn: &Connection) -> (domain::TaskId, String) {
        let id = store::tasks::create(
            conn,
            "t",
            TaskKind::Implement,
            None,
            &[],
            None,
            &domain::TaskOrigin::Idea,
        )
        .unwrap();
        store::tasks::set_ready(conn, &id).unwrap();
        let session_id = id.session_id().to_string();
        assert!(store::tasks::claim_for_dispatch(conn, &id, &session_id).unwrap());
        (id, session_id)
    }

    fn status_of(tasks: &[Task], id: &domain::TaskId) -> TaskStatus {
        tasks
            .iter()
            .find(|t| t.id.to_string() == id.to_string())
            .expect("task present in returned list")
            .status
    }

    #[test]
    fn poll_sessions_inner_completes_idle_in_progress_task() {
        let conn = Connection::open_in_memory().unwrap();
        store::tasks::ensure_table(&conn).unwrap();
        let (id, session_id) = dispatched_task(&conn);

        let dir = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let updated_ms = (now - Duration::seconds(31)).timestamp_millis();
        std::fs::write(
            dir.path().join("1.json"),
            format!(r#"{{"sessionId":"{session_id}","status":"idle","updatedAt":{updated_ms}}}"#),
        )
        .unwrap();

        let tasks =
            poll_sessions_inner(&conn, dir.path(), now, &PollThresholds::production()).unwrap();
        assert_eq!(status_of(&tasks, &id), TaskStatus::InReview);
    }

    #[test]
    fn poll_sessions_inner_leaves_busy_in_progress_task() {
        let conn = Connection::open_in_memory().unwrap();
        store::tasks::ensure_table(&conn).unwrap();
        let (id, session_id) = dispatched_task(&conn);

        let dir = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let updated_ms = (now - Duration::seconds(60)).timestamp_millis();
        std::fs::write(
            dir.path().join("1.json"),
            format!(r#"{{"sessionId":"{session_id}","status":"busy","updatedAt":{updated_ms}}}"#),
        )
        .unwrap();

        let tasks =
            poll_sessions_inner(&conn, dir.path(), now, &PollThresholds::production()).unwrap();
        assert_eq!(status_of(&tasks, &id), TaskStatus::InProgress);
    }
}
