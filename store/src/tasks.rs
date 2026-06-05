use anyhow::{Context, Result};
use chrono::Utc;
use domain::{Task, TaskId, TaskKind, TaskStatus};
use rusqlite::{params, Connection, OptionalExtension};

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
/// (issue_links / pr_links / doc_links) or the old 'general' kind variant is detected.
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

    let needs_migration = match &schema {
        None => false,
        Some(s) => s.contains("issue_links") || s.contains("'general'"),
    };

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
        .filter_map(|r| r.ok())
        .any(|name| name == column);
    if !exists {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {def};"))
            .with_context(|| format!("ALTER TABLE {table} ADD COLUMN {column}"))?;
    }
    Ok(())
}

/// Inserts a new task in `backlog` status and returns its generated `TaskId`.
pub fn create(
    conn: &Connection,
    title: &str,
    kind: TaskKind,
    description: Option<&str>,
    links: &[String],
) -> Result<TaskId> {
    let now = Utc::now().to_rfc3339();
    let links_str = links_to_db(links);
    conn.execute(
        "INSERT INTO tasks (title, kind, description, links, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![title, kind.to_string(), description, links_str, now],
    )
    .context("failed to insert task")?;
    let row_id = conn.last_insert_rowid();
    Ok(TaskId::from_db(format!("TASK-{row_id:04}")))
}

/// Transitions a task from `backlog` to `ready`. Returns an error if the task
/// does not exist or is not currently in `backlog` status.
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
            "task {} is not in 'backlog' status (may not exist or already transitioned)",
            id
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
pub fn list_visible(conn: &Connection) -> Result<Vec<Task>> {
    let done_cutoff = (Utc::now() - chrono::Duration::days(TERMINAL_VISIBLE_DAYS)).to_rfc3339();
    let mut stmt = conn
        .prepare(
            "SELECT id, title, status, kind, session_id,
                    description, links, created_at,
                    COALESCE(updated_at, created_at)
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
    ) = conn
        .query_row(
            "SELECT title, description, status, kind, session_id,
                    links, created_at,
                    COALESCE(updated_at, created_at)
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
        links: links_from_db(links_str),
        created_at: created_at_str,
        updated_at: updated_at_opt,
        age,
        urgency,
        comments,
    })
}

/// Updates the status of a task and refreshes `updated_at`.
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
        create(&conn, "Persistent task", TaskKind::Implement, None, &[]).unwrap();
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
        let id = create(&conn, "Fix auth bug", TaskKind::Implement, None, &[]).unwrap();
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
        let id1 = create(&conn, "First task", TaskKind::Implement, None, &[]).unwrap();
        let id2 = create(&conn, "Second task", TaskKind::Debug, None, &[]).unwrap();
        assert_eq!(id1.to_string(), "TASK-0001");
        assert_eq!(id2.to_string(), "TASK-0002");
    }

    #[test]
    fn set_ready_transitions_status_from_backlog() {
        let conn = in_memory();
        let id = create(&conn, "Fix auth bug", TaskKind::Implement, None, &[]).unwrap();
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
        let id = create(&conn, "Fix auth bug", TaskKind::Implement, None, &[]).unwrap();
        set_ready(&conn, &id).unwrap();
        let result = set_ready(&conn, &id);
        assert!(result.is_err(), "expected error when task is already ready");
    }

    #[test]
    fn list_visible_includes_all_active_statuses() {
        let conn = in_memory();
        for status in &["backlog", "ready", "in-progress", "blocked", "in-review"] {
            conn.execute(
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
            conn.execute(
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
            conn.execute(
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
        let id = create(&conn, "Fix auth bug", TaskKind::Implement, None, &[]).unwrap();
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
        create(&conn, "No comments task", TaskKind::Debug, None, &[]).unwrap();
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
        )
        .unwrap();
        let task = get(&conn, &id).unwrap();
        assert_eq!(task.links, vec!["https://github.com/owner/repo/issues/1"]);
    }

    #[test]
    fn create_without_optional_fields_returns_empty_defaults() {
        let conn = in_memory();
        let id = create(&conn, "Task", TaskKind::Implement, None, &[]).unwrap();
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
        let id = create(&conn, "Task", TaskKind::Implement, None, &[]).unwrap();
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
        let id = create(&conn, "Task", TaskKind::Implement, None, &[]).unwrap();
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
        let id = create(&conn, "Task", TaskKind::Implement, None, &[]).unwrap();
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
}
