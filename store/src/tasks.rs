use anyhow::{Context, Result};
use chrono::Utc;
use domain::{ReadyTask, RepoSlug, SignalIdentity, Task, TaskId, TaskKind, TaskOrigin, TaskStatus};
use rusqlite::{params, Connection, OptionalExtension};

/// Ensures the `tasks` and `task_comments` tables exist, running any pending migrations.
///
/// # Errors
/// Returns an error if a migration or schema creation statement fails.
pub fn ensure_table(conn: &Connection) -> Result<()> {
    migrate_consolidate_links_and_kind(conn)?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tasks (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL,
            description TEXT,
            status      TEXT NOT NULL DEFAULT 'backlog'
                        CHECK(status IN ('backlog','ready','in-progress','blocked','in-review','done','failed','cancelled')),
            kind        TEXT NOT NULL DEFAULT 'implement'
                        CHECK(kind IN ('review','implement','debug')),
            session_id  TEXT,
            links       TEXT,
            created_at  TEXT NOT NULL,
            updated_at  TEXT
        );",
    )
    .context("failed to ensure tasks schema")?;

    crate::task_comments::ensure_table(conn)?;

    for (col, def) in &[
        ("description", "TEXT"),
        ("updated_at", "TEXT"),
        ("links", "TEXT"),
        ("repo", "TEXT"),
        ("origin", "TEXT"),
    ] {
        add_column_if_missing(conn, "tasks", col, def)?;
    }

    // Rename old status values. PRAGMA ignore_check_constraints lets us write the new
    // values even though the existing table's CHECK still lists the old names.
    conn.execute_batch(
        "PRAGMA ignore_check_constraints = ON;
         UPDATE tasks SET status = 'cancelled' WHERE status = 'archived';
         UPDATE tasks SET status = 'in-review' WHERE status = 'review';
         PRAGMA ignore_check_constraints = OFF;",
    )
    .context("failed to migrate status renames (archived→cancelled, review→in-review)")?;

    Ok(())
}

/// Drops and recreates the tasks tables when the old multi-column link schema
/// (`issue_links` / `pr_links` / `doc_links`) or the old 'general' kind variant is detected.
/// All data is test data — drop and regenerate is safe.
/// Idempotent: no-op when the new schema is already in place.
fn migrate_consolidate_links_and_kind(conn: &Connection) -> Result<()> {
    let schema: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='tasks'",
            [],
            |row| row.get(0),
        )
        .optional()
        .context("failed to read tasks schema from sqlite_master")?;

    let needs_migration = schema
        .as_ref()
        .is_some_and(|s| s.contains("issue_links") || s.contains("'general'"));

    if !needs_migration {
        return Ok(());
    }

    conn.execute_batch(
        "DROP TABLE IF EXISTS task_comments;
         DROP TABLE IF EXISTS tasks;",
    )
    .context("failed to drop tasks tables for schema migration")?;

    Ok(())
}

fn add_column_if_missing(conn: &Connection, table: &str, column: &str, def: &str) -> Result<()> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .with_context(|| format!("PRAGMA table_info({table})"))?;

    let exists = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .context("failed to read table_info")?
        .filter_map(std::result::Result::ok)
        .any(|name| name == column);

    if !exists {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {def};"))
            .with_context(|| format!("ALTER TABLE {table} ADD COLUMN {column}"))?;
    }

    Ok(())
}

/// Inserts a new task in `backlog` status and returns its generated `TaskId`.
///
/// # Errors
/// Returns an error if the SQL statement fails.
pub fn create(
    conn: &Connection,
    title: &str,
    kind: TaskKind,
    description: Option<&str>,
    links: &[String],
    repo: Option<&str>,
    origin: &TaskOrigin,
) -> Result<TaskId> {
    let now = Utc::now().to_rfc3339();
    let links_str = links_to_db(links);
    let origin_str = origin_to_db(origin)?;

    let _ = conn
        .execute(
            "INSERT INTO tasks (title, kind, description, links, repo, origin, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                title,
                kind.to_string(),
                description,
                links_str,
                repo,
                origin_str,
                now
            ],
        )
        .context("failed to insert task")?;

    let row_id = conn.last_insert_rowid();

    Ok(TaskId::from_db(format!("TASK-{row_id:04}")))
}

/// Transitions a task from `backlog` to `ready`. Returns an error if the task
/// does not exist or is not currently in `backlog` status.
///
/// # Errors
/// Returns an error if the task does not exist, is not in `backlog` status, or the SQL statement fails.
pub fn set_ready(conn: &Connection, id: &TaskId) -> Result<()> {
    let row_id = task_row_id(id)?;

    let changed = conn
        .execute(
            "UPDATE tasks SET status = 'ready' WHERE id = ?1 AND status = 'backlog'",
            params![row_id],
        )
        .context("failed to update task status")?;

    if changed == 0 {
        anyhow::bail!(
            "task {id} is not in 'backlog' status (may not exist or already transitioned)"
        );
    }

    Ok(())
}

/// Terminal tasks (`done`, `failed`, `cancelled`) remain visible for this many days so
/// accidental transitions can be caught and reversed before the task disappears.
const TERMINAL_VISIBLE_DAYS: i64 = 7;

/// Returns tasks that appear in the TUI unified list: all non-terminal statuses
/// plus `done`, `failed`, and `cancelled` tasks updated within the last
/// [`TERMINAL_VISIBLE_DAYS`] days.
///
/// # Errors
/// Returns an error if the SQL query fails or a row value cannot be parsed.
pub fn list_visible(conn: &Connection) -> Result<Vec<Task>> {
    let done_cutoff = (Utc::now() - chrono::Duration::days(TERMINAL_VISIBLE_DAYS)).to_rfc3339();

    let mut stmt = conn
        .prepare(
            "SELECT id, title, status, kind, session_id,
                    description, links, created_at,
                    COALESCE(updated_at, created_at), repo, origin
             FROM tasks
             WHERE status IN ('backlog', 'ready', 'in-progress', 'blocked', 'in-review')
                OR (status IN ('done', 'failed', 'cancelled') AND COALESCE(updated_at, created_at) >= ?1)
             ORDER BY created_at ASC",
        )
        .context("failed to prepare task query")?;

    let rows = stmt
        .query_map(params![done_cutoff], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
            ))
        })
        .context("failed to query tasks")?
        .map(|r| r.context("failed to read task row"))
        .collect::<Result<Vec<_>>>()?;

    let now = Utc::now();

    rows.into_iter()
        .map(
            |(
                id,
                title,
                status_str,
                kind_str,
                session_id,
                description,
                links_str,
                created_at_str,
                updated_at_str,
                repo_str,
                origin_str,
            )| {
                let task_id = TaskId::from_db(format!("TASK-{id:04}"));

                let status: TaskStatus = status_str
                    .parse()
                    .map_err(|e: String| anyhow::anyhow!(e))
                    .with_context(|| format!("invalid status for task {id}"))?;

                let kind: TaskKind = kind_str
                    .parse()
                    .map_err(|e: String| anyhow::anyhow!(e))
                    .with_context(|| format!("invalid kind for task {id}"))?;

                let repo = repo_str
                    .map(|s| {
                        s.parse::<RepoSlug>()
                            .map_err(|e: String| anyhow::anyhow!(e))
                            .with_context(|| format!("invalid repo slug for task {id}"))
                    })
                    .transpose()?;

                let origin = origin_from_db(origin_str)
                    .with_context(|| format!("invalid origin for task {id}"))?;

                let urgency = status.urgency();

                let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                    .with_context(|| format!("invalid created_at for task {id}: {created_at_str}"))?
                    .with_timezone(&Utc);

                let age = now - created_at;

                let comments = crate::task_comments::for_task(conn, id)?;

                Ok(Task {
                    id: task_id,
                    title,
                    description,
                    status,
                    kind,
                    session_id,
                    repo,
                    origin,
                    links: links_from_db(links_str),
                    updated_at: updated_at_str,
                    created_at: created_at_str,
                    age,
                    urgency,
                    comments,
                })
            },
        )
        .collect()
}

/// Returns the full task including all fields and its comment thread.
///
/// # Errors
/// Returns an error if the task does not exist or a row value cannot be parsed.
pub fn get(conn: &Connection, id: &TaskId) -> Result<Task> {
    let row_id = task_row_id(id)?;
    let (
        title,
        description,
        status_str,
        kind_str,
        session_id,
        links_str,
        created_at_str,
        updated_at_opt,
        repo_str,
        origin_str,
    ) = conn
        .query_row(
            "SELECT title, description, status, kind, session_id,
                    links, created_at,
                    COALESCE(updated_at, created_at), repo, origin
             FROM tasks WHERE id = ?1",
            params![row_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .with_context(|| format!("task {id} not found"))?;

    let status: TaskStatus = status_str
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))
        .with_context(|| format!("invalid status for task {id}"))?;

    let kind: TaskKind = kind_str
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))
        .with_context(|| format!("invalid kind for task {id}"))?;

    let repo = repo_str
        .map(|s| {
            s.parse::<RepoSlug>()
                .map_err(|e: String| anyhow::anyhow!(e))
                .with_context(|| format!("invalid repo slug for task {id}"))
        })
        .transpose()?;

    let origin =
        origin_from_db(origin_str).with_context(|| format!("invalid origin for task {id}"))?;

    let urgency = status.urgency();

    let created_at_dt = chrono::DateTime::parse_from_rfc3339(&created_at_str)
        .with_context(|| format!("invalid created_at for task {id}"))?
        .with_timezone(&Utc);

    let age = Utc::now() - created_at_dt;

    let comments = crate::task_comments::for_task(conn, row_id)?;

    Ok(Task {
        id: id.clone(),
        title,
        description,
        status,
        kind,
        session_id,
        repo,
        origin,
        links: links_from_db(links_str),
        created_at: created_at_str,
        updated_at: updated_at_opt,
        age,
        urgency,
        comments,
    })
}

/// Updates the status of a task and refreshes `updated_at`.
///
/// # Errors
/// Returns an error if the task does not exist or the SQL statement fails.
pub fn update_status(conn: &Connection, id: &TaskId, status: TaskStatus) -> Result<()> {
    let row_id = task_row_id(id)?;

    let now = Utc::now().to_rfc3339();

    let changed = conn
        .execute(
            "UPDATE tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.to_string(), now, row_id],
        )
        .context("failed to update task status")?;

    if changed == 0 {
        anyhow::bail!("task {id} not found");
    }

    Ok(())
}

/// Returns the non-terminal task whose origin matches `origin`, or `None` if no
/// such task exists. Always returns `None` for `TaskOrigin::Idea` — Idea tasks
/// have no signal to match against.
///
/// Used by the task creation pre-flight check to enforce one-active-task-per-signal.
///
/// # Errors
/// Propagates any error from `list_visible`.
pub fn active_for_origin(conn: &Connection, origin: &TaskOrigin) -> Result<Option<Task>> {
    if matches!(origin, TaskOrigin::Idea) {
        return Ok(None);
    }
    let target = SignalIdentity::from(origin);
    let tasks = list_visible(conn)?;
    Ok(tasks
        .into_iter()
        .find(|t| !t.status.is_terminal() && SignalIdentity::from(&t.origin) == target))
}

/// Returns the count of tasks currently in `in-progress` status.
///
/// # Errors
/// Returns an error if the SQL query fails.
pub fn count_in_progress(conn: &Connection) -> Result<u32> {
    conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE status = 'in-progress'",
        [],
        |row| row.get(0),
    )
    .context("failed to count in-progress tasks")
}

/// Returns the oldest `ready` task by `created_at`, or `None` if none exist.
///
/// # Errors
/// Returns an error if the SQL query fails or a row value cannot be parsed.
pub fn oldest_ready(conn: &Connection) -> Result<Option<ReadyTask>> {
    conn.query_row(
        "SELECT id, repo, kind FROM tasks WHERE status = 'ready' ORDER BY created_at ASC LIMIT 1",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )
    .optional()
    .context("failed to query oldest ready task")?
    .map(|(id, repo_str, kind_str)| {
        let task_id = TaskId::from_db(format!("TASK-{id:04}"));

        let kind: TaskKind = kind_str
            .parse()
            .map_err(|e: String| anyhow::anyhow!(e))
            .with_context(|| format!("invalid kind for task {id}"))?;

        let repo = repo_str
            .map(|s| {
                s.parse::<RepoSlug>()
                    .map_err(|e: String| anyhow::anyhow!(e))
                    .with_context(|| format!("invalid repo slug for task {id}"))
            })
            .transpose()?;

        Ok(ReadyTask {
            id: task_id,
            repo,
            kind,
        })
    })
    .transpose()
}

/// Atomically transitions a task from `ready` to `in-progress` and writes the session ID.
///
/// Returns `true` if the claim succeeded, `false` if the task was already
/// claimed by a concurrent dispatch tick (rowcount == 0).
///
/// # Errors
/// Returns an error if the SQL statement fails.
pub fn claim_for_dispatch(conn: &Connection, id: &TaskId, session_id: &str) -> Result<bool> {
    let row_id = task_row_id(id)?;

    let now = Utc::now().to_rfc3339();

    let changed = conn
        .execute(
            "UPDATE tasks SET status = 'in-progress', session_id = ?1, updated_at = ?2
             WHERE id = ?3 AND status = 'ready'",
            params![session_id, now, row_id],
        )
        .context("failed to claim task for dispatch")?;

    Ok(changed == 1)
}

/// Appends `value` to the task's `links` field. Idempotent: if `value` is
/// already present, the field is left unchanged. Refreshes `updated_at`.
///
/// # Race window
/// This is a read-modify-write on a TEXT CSV column. Concurrent callers could
/// each read the same list, both find the value absent, and both append it.
/// In practice the only caller is `hub task link` (a single-process CLI), so
/// this race cannot occur. If `links` moves to a normalised table this concern
/// disappears entirely.
///
/// # Errors
/// Returns an error if the task does not exist or the SQL statement fails.
pub fn add_link(conn: &Connection, id: &TaskId, value: &str) -> Result<()> {
    let row_id = task_row_id(id)?;
    let links_str: Option<String> = conn
        .query_row(
            "SELECT links FROM tasks WHERE id = ?1",
            params![row_id],
            |row| row.get(0),
        )
        .with_context(|| format!("task {id} not found"))?;

    let mut links = links_from_db(links_str);

    if links.iter().any(|l| l == value) {
        return Ok(());
    }

    links.push(value.to_string());

    let now = Utc::now().to_rfc3339();

    let _ = conn
        .execute(
            "UPDATE tasks SET links = ?1, updated_at = ?2 WHERE id = ?3",
            params![links_to_db(&links), now, row_id],
        )
        .context("failed to update task links")?;

    Ok(())
}

fn links_to_db(links: &[String]) -> Option<String> {
    if links.is_empty() {
        None
    } else {
        Some(links.join(","))
    }
}

fn links_from_db(val: Option<String>) -> Vec<String> {
    match val {
        None => vec![],
        Some(s) if s.is_empty() => vec![],
        Some(s) => s.split(',').map(str::to_string).collect(),
    }
}

/// Serializes an origin to its JSON column value. New rows always store the
/// JSON (including `{"type":"idea"}`); a NULL column means strictly "historical
/// row created before origins existed".
fn origin_to_db(origin: &TaskOrigin) -> Result<String> {
    serde_json::to_string(origin).context("failed to serialize task origin")
}

/// Parses an origin from its JSON column value. A NULL or empty column (a
/// historical row) reads as `Idea`.
fn origin_from_db(val: Option<String>) -> Result<TaskOrigin> {
    match val {
        None => Ok(TaskOrigin::default()),
        Some(s) if s.is_empty() => Ok(TaskOrigin::default()),
        Some(s) => serde_json::from_str(&s).context("failed to deserialize task origin"),
    }
}

pub(crate) fn task_row_id(id: &TaskId) -> Result<i64> {
    let s = id.to_string();
    s[5..]
        .parse::<i64>()
        .with_context(|| format!("malformed task ID: {id}"))
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
    fn ensure_table_creates_expected_columns() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        let column_names: Vec<String> = conn
            .prepare("PRAGMA table_info(tasks)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for col in &["description", "links", "updated_at"] {
            assert!(
                column_names.contains(&col.to_string()),
                "missing column: {col}"
            );
        }
        for old_col in &["issue_links", "pr_links", "doc_links"] {
            assert!(
                !column_names.contains(&old_col.to_string()),
                "old column should not exist: {old_col}"
            );
        }
    }

    #[test]
    fn ensure_table_drops_old_link_columns_and_recreates() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                title       TEXT NOT NULL,
                description TEXT,
                status      TEXT NOT NULL DEFAULT 'backlog',
                kind        TEXT NOT NULL DEFAULT 'implement',
                session_id  TEXT,
                issue_links TEXT,
                pr_links    TEXT,
                doc_links   TEXT,
                created_at  TEXT NOT NULL,
                updated_at  TEXT
            );",
        )
        .unwrap();
        ensure_table(&conn).unwrap();
        let column_names: Vec<String> = conn
            .prepare("PRAGMA table_info(tasks)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(column_names.contains(&"links".to_string()));
        assert!(!column_names.contains(&"issue_links".to_string()));
    }

    #[test]
    fn ensure_table_is_idempotent_and_does_not_drop_existing_tasks() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        let _ = create(
            &conn,
            "Persistent task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        ensure_table(&conn).unwrap();
        let tasks = list_visible(&conn).unwrap();
        assert_eq!(
            tasks.len(),
            1,
            "ensure_table must not drop tasks on second call"
        );
    }

    #[test]
    fn ensure_table_creates_task_comments_table() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM task_comments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn create_task_returns_key_with_task_prefix_format() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Fix auth bug",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        let s = id.to_string();
        assert!(s.starts_with("TASK-"), "expected TASK- prefix, got {s}");
        assert!(
            s[5..].chars().all(|c| c.is_ascii_digit()),
            "expected digits after TASK-, got {s}"
        );
    }

    #[test]
    fn create_multiple_tasks_produces_sequential_keys() {
        let conn = in_memory();
        let id1 = create(
            &conn,
            "First task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        let id2 = create(
            &conn,
            "Second task",
            TaskKind::Debug,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        assert_eq!(id1.to_string(), "TASK-0001");
        assert_eq!(id2.to_string(), "TASK-0002");
    }

    #[test]
    fn set_ready_transitions_status_from_backlog() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Fix auth bug",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        set_ready(&conn, &id).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status = 'ready'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn set_ready_fails_when_status_is_not_backlog() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Fix auth bug",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        set_ready(&conn, &id).unwrap();
        let result = set_ready(&conn, &id);
        assert!(result.is_err(), "expected error when task is already ready");
    }

    #[test]
    fn list_visible_includes_all_active_statuses() {
        let conn = in_memory();
        for status in &["backlog", "ready", "in-progress", "blocked", "in-review"] {
            let _ = conn.execute(
                "INSERT INTO tasks (title, status, kind, created_at) VALUES (?1, ?2, 'implement', ?3)",
                params![format!("task {status}"), status, Utc::now().to_rfc3339()],
            )
            .unwrap();
        }
        let visible = list_visible(&conn).unwrap();
        assert_eq!(visible.len(), 5);
        let statuses: Vec<String> = visible.iter().map(|t| t.status.to_string()).collect();
        for expected in &["backlog", "ready", "in-progress", "blocked", "in-review"] {
            assert!(
                statuses.contains(&expected.to_string()),
                "missing: {expected}"
            );
        }
    }

    #[test]
    fn list_visible_includes_recently_terminal_tasks() {
        let conn = in_memory();
        let recent = Utc::now().to_rfc3339();
        for status in &["done", "failed", "cancelled"] {
            let _ = conn.execute(
                "INSERT INTO tasks (title, status, kind, created_at, updated_at) VALUES (?1, ?2, 'implement', ?3, ?3)",
                params![format!("{status} task"), status, recent],
            )
            .unwrap();
        }
        let visible = list_visible(&conn).unwrap();
        assert_eq!(visible.len(), 3);
        let statuses: Vec<String> = visible.iter().map(|t| t.status.to_string()).collect();
        assert!(statuses.contains(&"done".to_string()));
        assert!(statuses.contains(&"failed".to_string()));
        assert!(statuses.contains(&"cancelled".to_string()));
    }

    #[test]
    fn list_visible_excludes_terminal_tasks_older_than_window() {
        let conn = in_memory();
        let old = (Utc::now() - chrono::Duration::days(TERMINAL_VISIBLE_DAYS + 1)).to_rfc3339();
        for status in &["done", "failed", "cancelled"] {
            let _ = conn.execute(
                "INSERT INTO tasks (title, status, kind, created_at, updated_at) VALUES (?1, ?2, 'implement', ?3, ?3)",
                params![format!("old {status}"), status, old],
            )
            .unwrap();
        }
        let visible = list_visible(&conn).unwrap();
        assert!(
            visible.is_empty(),
            "terminal tasks older than window should be excluded"
        );
    }

    #[test]
    fn list_visible_returns_empty_when_no_visible_tasks() {
        let conn = in_memory();
        let tasks = list_visible(&conn).unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn list_visible_populates_comments_from_db() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Fix auth bug",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        crate::task_comments::add(&conn, &id, domain::CommentAuthor::Agent, "Found the issue")
            .unwrap();
        let tasks = list_visible(&conn).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].comments.len(), 1);
        assert_eq!(tasks[0].comments[0].content, "Found the issue");
        assert!(matches!(
            tasks[0].comments[0].author,
            domain::CommentAuthor::Agent
        ));
    }

    #[test]
    fn list_visible_returns_empty_comments_when_none_added() {
        let conn = in_memory();
        let _ = create(
            &conn,
            "No comments task",
            TaskKind::Debug,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        let tasks = list_visible(&conn).unwrap();
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].comments.is_empty());
    }

    // ── create (extended) ─────────────────────────────────────────────────────

    #[test]
    fn create_with_description_stores_and_returns_via_get() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Fix auth bug",
            TaskKind::Implement,
            Some("The OAuth token is not being refreshed"),
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        let task = get(&conn, &id).unwrap();
        assert_eq!(
            task.description.as_deref(),
            Some("The OAuth token is not being refreshed")
        );
    }

    #[test]
    fn create_with_link_stores_and_returns_via_get() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Fix auth bug",
            TaskKind::Implement,
            None,
            &["https://github.com/owner/repo/issues/1".to_string()],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        let task = get(&conn, &id).unwrap();
        assert_eq!(task.links, vec!["https://github.com/owner/repo/issues/1"]);
    }

    #[test]
    fn create_without_optional_fields_returns_empty_defaults() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        let task = get(&conn, &id).unwrap();
        assert!(task.description.is_none());
        assert!(task.links.is_empty());
        assert!(task.comments.is_empty());
    }

    // ── get ───────────────────────────────────────────────────────────────────

    #[test]
    fn get_errors_on_nonexistent_task() {
        let conn = in_memory();
        let id: TaskId = "TASK-9999".parse().unwrap();
        assert!(get(&conn, &id).is_err());
    }

    // ── update_status ─────────────────────────────────────────────────────────

    #[test]
    fn update_status_changes_status_and_refreshes_updated_at() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        let before = get(&conn, &id).unwrap();
        update_status(&conn, &id, TaskStatus::InReview).unwrap();
        let after = get(&conn, &id).unwrap();
        assert_eq!(after.status, TaskStatus::InReview);
        assert!(after.updated_at >= before.updated_at);
    }

    #[test]
    fn update_status_errors_on_nonexistent_task() {
        let conn = in_memory();
        let id: TaskId = "TASK-9999".parse().unwrap();
        assert!(update_status(&conn, &id, TaskStatus::InReview).is_err());
    }

    // ── add_comment ───────────────────────────────────────────────────────────

    #[test]
    fn add_comment_appends_in_chronological_order() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        crate::task_comments::add(
            &conn,
            &id,
            domain::CommentAuthor::Agent,
            "Starting investigation",
        )
        .unwrap();
        crate::task_comments::add(&conn, &id, domain::CommentAuthor::Agent, "Found the issue")
            .unwrap();
        let task = get(&conn, &id).unwrap();
        assert_eq!(task.comments.len(), 2);
        assert_eq!(task.comments[0].content, "Starting investigation");
        assert_eq!(task.comments[1].content, "Found the issue");
    }

    #[test]
    fn add_comment_updates_task_updated_at() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        let before = get(&conn, &id).unwrap();
        crate::task_comments::add(&conn, &id, domain::CommentAuthor::Agent, "Progress note")
            .unwrap();
        let after = get(&conn, &id).unwrap();
        assert!(after.updated_at >= before.updated_at);
    }

    #[test]
    fn add_comment_errors_on_nonexistent_task() {
        let conn = in_memory();
        let id: TaskId = "TASK-9999".parse().unwrap();
        assert!(
            crate::task_comments::add(&conn, &id, domain::CommentAuthor::Agent, "note").is_err()
        );
    }

    // ── repo field ────────────────────────────────────────────────────────────

    #[test]
    fn create_with_repo_stores_and_retrieves_slug() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Fix CI",
            TaskKind::Debug,
            None,
            &[],
            Some("ooloth/hub"),
            &TaskOrigin::Idea,
        )
        .unwrap();
        let task = get(&conn, &id).unwrap();
        assert_eq!(
            task.repo.as_ref().map(|r| r.to_string()).as_deref(),
            Some("ooloth/hub")
        );
    }

    #[test]
    fn create_without_repo_retrieves_none() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        let task = get(&conn, &id).unwrap();
        assert!(task.repo.is_none());
    }

    // ── origin field ──────────────────────────────────────────────────────────

    #[test]
    fn ensure_table_creates_origin_column() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        let column_names: Vec<String> = conn
            .prepare("PRAGMA table_info(tasks)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            column_names.contains(&"origin".to_string()),
            "missing column: origin"
        );
    }

    #[test]
    fn create_round_trips_typed_origin_via_get() {
        let conn = in_memory();
        let origin = TaskOrigin::Pr {
            repo: RepoSlug::new("ooloth", "hub"),
            number: 42,
        };
        let id = create(
            &conn,
            "Review PR",
            TaskKind::Review,
            None,
            &[],
            None,
            &origin,
        )
        .unwrap();
        assert_eq!(get(&conn, &id).unwrap().origin, origin);
    }

    #[test]
    fn list_visible_round_trips_typed_origin() {
        let conn = in_memory();
        let origin = TaskOrigin::Alert {
            source: domain::AlertSource::Loki,
            key: "proj/prod/db error".into(),
            label: "proj:prod — errors".into(),
        };
        let _ = create(
            &conn,
            "Investigate alert",
            TaskKind::Debug,
            None,
            &[],
            None,
            &origin,
        )
        .unwrap();
        let tasks = list_visible(&conn).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].origin, origin);
    }

    #[test]
    fn get_reads_null_origin_as_idea() {
        // Rows created before the origin column existed have a NULL origin and
        // must read back as `Idea`, not error.
        let conn = in_memory();
        let _ = conn.execute(
            "INSERT INTO tasks (title, status, kind, created_at) VALUES ('old', 'backlog', 'implement', ?1)",
            params![Utc::now().to_rfc3339()],
        )
        .unwrap();
        let id = TaskId::from_db(format!("TASK-{:04}", conn.last_insert_rowid()));
        assert_eq!(get(&conn, &id).unwrap().origin, TaskOrigin::Idea);
    }

    #[test]
    fn list_visible_reads_null_origin_as_idea() {
        let conn = in_memory();
        let _ = conn.execute(
            "INSERT INTO tasks (title, status, kind, created_at) VALUES ('old', 'backlog', 'implement', ?1)",
            params![Utc::now().to_rfc3339()],
        )
        .unwrap();
        let tasks = list_visible(&conn).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].origin, TaskOrigin::Idea);
    }

    // ── count_in_progress ─────────────────────────────────────────────────────

    #[test]
    fn count_in_progress_returns_correct_count() {
        let conn = in_memory();
        assert_eq!(count_in_progress(&conn).unwrap(), 0);
        let _ = conn.execute(
            "INSERT INTO tasks (title, status, kind, created_at) VALUES ('t', 'in-progress', 'implement', ?1)",
            params![Utc::now().to_rfc3339()],
        ).unwrap();
        assert_eq!(count_in_progress(&conn).unwrap(), 1);
        let _ = conn.execute(
            "INSERT INTO tasks (title, status, kind, created_at) VALUES ('t2', 'ready', 'implement', ?1)",
            params![Utc::now().to_rfc3339()],
        ).unwrap();
        assert_eq!(count_in_progress(&conn).unwrap(), 1);
    }

    // ── oldest_ready ──────────────────────────────────────────────────────────

    #[test]
    fn oldest_ready_returns_none_when_no_ready_tasks() {
        let conn = in_memory();
        assert!(oldest_ready(&conn).unwrap().is_none());
    }

    #[test]
    fn oldest_ready_returns_earliest_by_created_at() {
        let conn = in_memory();
        let older = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let newer = Utc::now().to_rfc3339();
        let _ = conn.execute(
            "INSERT INTO tasks (title, status, kind, created_at) VALUES ('newer', 'ready', 'debug', ?1)",
            params![newer],
        ).unwrap();
        let _ = conn.execute(
            "INSERT INTO tasks (title, status, kind, created_at) VALUES ('older', 'ready', 'implement', ?1)",
            params![older],
        ).unwrap();
        let task = oldest_ready(&conn).unwrap().unwrap();
        assert_eq!(task.kind, TaskKind::Implement);
    }

    #[test]
    fn oldest_ready_populates_repo_when_set() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Task",
            TaskKind::Implement,
            None,
            &[],
            Some("ooloth/hub"),
            &TaskOrigin::Idea,
        )
        .unwrap();
        set_ready(&conn, &id).unwrap();
        let task = oldest_ready(&conn).unwrap().unwrap();
        assert_eq!(
            task.repo.as_ref().map(|r| r.to_string()).as_deref(),
            Some("ooloth/hub")
        );
    }

    // ── claim_for_dispatch ────────────────────────────────────────────────────

    #[test]
    fn claim_for_dispatch_transitions_to_in_progress_and_sets_session_id() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        set_ready(&conn, &id).unwrap();
        let claimed = claim_for_dispatch(&conn, &id, "test-session-uuid").unwrap();
        assert!(claimed);
        let task = get(&conn, &id).unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.session_id.as_deref(), Some("test-session-uuid"));
    }

    #[test]
    fn claim_for_dispatch_returns_false_when_already_in_progress() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        set_ready(&conn, &id).unwrap();
        let _ = claim_for_dispatch(&conn, &id, "first").unwrap();
        let second = claim_for_dispatch(&conn, &id, "second").unwrap();
        assert!(!second);
    }

    #[test]
    fn claim_for_dispatch_returns_false_for_nonexistent_task() {
        let conn = in_memory();
        let id: TaskId = "TASK-9999".parse().unwrap();
        let claimed = claim_for_dispatch(&conn, &id, "uuid").unwrap();
        assert!(!claimed);
    }

    // ── add_link ──────────────────────────────────────────────────────────────

    #[test]
    fn add_link_appends_new_link() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        add_link(&conn, &id, "https://github.com/owner/repo/pull/1").unwrap();
        let task = get(&conn, &id).unwrap();
        assert_eq!(task.links, vec!["https://github.com/owner/repo/pull/1"]);
    }

    #[test]
    fn add_link_is_idempotent_for_duplicate() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        let url = "https://github.com/owner/repo/pull/1";
        add_link(&conn, &id, url).unwrap();
        add_link(&conn, &id, url).unwrap();
        let task = get(&conn, &id).unwrap();
        assert_eq!(task.links.len(), 1, "duplicate link must not be added");
    }

    #[test]
    fn add_link_accumulates_distinct_links() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        add_link(&conn, &id, "https://example.com/a").unwrap();
        add_link(&conn, &id, "https://example.com/b").unwrap();
        add_link(&conn, &id, "~/.hub/agent-session-logs/TASK-0001-fix.md").unwrap();
        let task = get(&conn, &id).unwrap();
        assert_eq!(
            task.links,
            vec![
                "https://example.com/a",
                "https://example.com/b",
                "~/.hub/agent-session-logs/TASK-0001-fix.md",
            ]
        );
    }

    #[test]
    fn add_link_refreshes_updated_at() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        let before = get(&conn, &id).unwrap();
        add_link(&conn, &id, "https://example.com").unwrap();
        let after = get(&conn, &id).unwrap();
        assert!(
            after.updated_at >= before.updated_at,
            "updated_at must be refreshed after add_link"
        );
    }

    #[test]
    fn add_link_errors_on_nonexistent_task() {
        let conn = in_memory();
        let id: TaskId = "TASK-9999".parse().unwrap();
        assert!(add_link(&conn, &id, "https://example.com").is_err());
    }

    // ── active_for_origin ─────────────────────────────────────────────────────

    fn pr_origin() -> TaskOrigin {
        TaskOrigin::Pr {
            repo: domain::RepoSlug::new("ooloth", "hub"),
            number: 42,
        }
    }

    #[test]
    fn active_for_origin_returns_non_terminal_task_with_matching_origin() {
        let conn = in_memory();
        let _ = create(
            &conn,
            "Fix it",
            TaskKind::Implement,
            None,
            &[],
            None,
            &pr_origin(),
        )
        .unwrap();
        let result = active_for_origin(&conn, &pr_origin()).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().origin, pr_origin());
    }

    #[test]
    fn active_for_origin_returns_none_when_matching_task_is_terminal() {
        let conn = in_memory();
        let id = create(
            &conn,
            "Done",
            TaskKind::Implement,
            None,
            &[],
            None,
            &pr_origin(),
        )
        .unwrap();
        update_status(&conn, &id, domain::TaskStatus::Done).unwrap();
        assert!(active_for_origin(&conn, &pr_origin()).unwrap().is_none());
    }

    #[test]
    fn active_for_origin_returns_none_for_idea_origin() {
        let conn = in_memory();
        let _ = create(
            &conn,
            "Idea task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &TaskOrigin::Idea,
        )
        .unwrap();
        assert!(active_for_origin(&conn, &TaskOrigin::Idea)
            .unwrap()
            .is_none());
    }

    #[test]
    fn active_for_origin_ignores_tasks_with_different_origin() {
        let conn = in_memory();
        let other = TaskOrigin::Pr {
            repo: domain::RepoSlug::new("ooloth", "hub"),
            number: 99,
        };
        let _ = create(
            &conn,
            "Other task",
            TaskKind::Implement,
            None,
            &[],
            None,
            &other,
        )
        .unwrap();
        assert!(active_for_origin(&conn, &pr_origin()).unwrap().is_none());
    }

    #[test]
    fn active_for_origin_matches_ci_origins_ignoring_url() {
        let conn = in_memory();
        let origin_at_creation = TaskOrigin::Ci {
            repo: domain::RepoSlug::new("ooloth", "hub"),
            workflow: "ci".into(),
            job: Some("test".into()),
            step: None,
            url: "https://github.com/actions/runs/1".into(),
        };
        let origin_at_lookup = TaskOrigin::Ci {
            repo: domain::RepoSlug::new("ooloth", "hub"),
            workflow: "ci".into(),
            job: Some("test".into()),
            step: None,
            url: "https://github.com/actions/runs/9999".into(), // different run
        };
        let _ = create(
            &conn,
            "Fix CI",
            TaskKind::Debug,
            None,
            &[],
            None,
            &origin_at_creation,
        )
        .unwrap();
        let result = active_for_origin(&conn, &origin_at_lookup).unwrap();
        assert!(result.is_some(), "should match despite different url");
    }
}
