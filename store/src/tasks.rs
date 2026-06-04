use anyhow::{Context, Result};
use chrono::Utc;
use domain::{CommentAuthor, Task, TaskComment, TaskId, TaskKind, TaskStatus};
use rusqlite::{params, Connection};

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tasks (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL,
            description TEXT,
            status      TEXT NOT NULL DEFAULT 'backlog'
                        CHECK(status IN ('backlog','ready','in-progress','blocked','review','done','archived')),
            kind        TEXT NOT NULL DEFAULT 'general'
                        CHECK(kind IN ('implement','debug','general')),
            session_id  TEXT,
            issue_links TEXT,
            pr_links    TEXT,
            doc_links   TEXT,
            created_at  TEXT NOT NULL,
            updated_at  TEXT
        );
        CREATE TABLE IF NOT EXISTS task_comments (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            author     TEXT    NOT NULL CHECK(author IN ('human', 'agent')),
            content    TEXT    NOT NULL,
            created_at TEXT    NOT NULL
        );",
    )
    .context("failed to ensure tasks schema")?;
    for (col, def) in &[
        ("description", "TEXT"),
        ("issue_links", "TEXT"),
        ("pr_links", "TEXT"),
        ("doc_links", "TEXT"),
        ("updated_at", "TEXT"),
    ] {
        add_column_if_missing(conn, "tasks", col, def)?;
    }
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
    issue_links: &[String],
) -> Result<TaskId> {
    let now = Utc::now().to_rfc3339();
    let issue_links_str = links_to_db(issue_links);
    conn.execute(
        "INSERT INTO tasks (title, kind, description, issue_links, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![title, kind.to_string(), description, issue_links_str, now],
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

/// Returns all tasks with status `in-progress`, `blocked`, or `review` —
/// the states that appear in the TUI unified list.
pub fn list_visible(conn: &Connection) -> Result<Vec<Task>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, status, kind, session_id, created_at
             FROM tasks
             WHERE status IN ('in-progress', 'blocked', 'review')
             ORDER BY created_at ASC",
        )
        .context("failed to prepare task query")?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .context("failed to query tasks")?
        .map(|r| r.context("failed to read task row"))
        .collect::<Result<Vec<_>>>()?;

    let now = Utc::now();
    rows.into_iter()
        .map(
            |(id, title, status_str, kind_str, session_id, created_at_str)| {
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
                Ok(Task {
                    id: task_id,
                    title,
                    description: None,
                    status,
                    kind,
                    session_id,
                    issue_links: vec![],
                    pr_links: vec![],
                    doc_links: vec![],
                    updated_at: created_at_str.clone(),
                    created_at: created_at_str,
                    age,
                    urgency,
                    comments: vec![],
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
        issue_links_str,
        pr_links_str,
        doc_links_str,
        created_at_str,
        updated_at_opt,
    ) = conn
        .query_row(
            "SELECT title, description, status, kind, session_id,
                    issue_links, pr_links, doc_links, created_at,
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
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
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

    let comments = get_comments(conn, row_id)?;

    Ok(Task {
        id: id.clone(),
        title,
        description,
        status,
        kind,
        session_id,
        issue_links: links_from_db(issue_links_str),
        pr_links: links_from_db(pr_links_str),
        doc_links: links_from_db(doc_links_str),
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

/// Appends a comment to a task and refreshes the task's `updated_at`.
pub fn add_comment(
    conn: &Connection,
    id: &TaskId,
    author: CommentAuthor,
    content: &str,
) -> Result<()> {
    let row_id = task_row_id(id)?;
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE id = ?1",
            params![row_id],
            |r| r.get::<_, i64>(0),
        )
        .context("failed to check task exists")?
        > 0;
    if !exists {
        anyhow::bail!("task {id} not found");
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO task_comments (task_id, author, content, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![row_id, author.to_string(), content, now],
    )
    .context("failed to insert task comment")?;
    conn.execute(
        "UPDATE tasks SET updated_at = ?1 WHERE id = ?2",
        params![now, row_id],
    )
    .context("failed to refresh task updated_at")?;
    Ok(())
}

fn get_comments(conn: &Connection, task_row_id: i64) -> Result<Vec<TaskComment>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, author, content, created_at
             FROM task_comments
             WHERE task_id = ?1
             ORDER BY created_at ASC",
        )
        .context("failed to prepare comments query")?;
    let rows: Vec<(i64, String, String, String)> = stmt
        .query_map(params![task_row_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .context("failed to query comments")?
        .map(|r| r.context("failed to read comment row"))
        .collect::<Result<_>>()?;
    rows.into_iter()
        .map(|(id, author_str, content, created_at)| {
            let author: CommentAuthor = author_str
                .parse()
                .map_err(|e: String| anyhow::anyhow!(e))
                .with_context(|| format!("invalid author in comment {id}"))?;
            Ok(TaskComment {
                id,
                author,
                content,
                created_at,
            })
        })
        .collect()
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

fn task_row_id(id: &TaskId) -> Result<i64> {
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
    fn ensure_table_adds_new_columns_to_existing_old_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                title      TEXT NOT NULL,
                status     TEXT NOT NULL DEFAULT 'backlog',
                kind       TEXT NOT NULL DEFAULT 'general',
                session_id TEXT,
                created_at TEXT NOT NULL
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
        for col in &[
            "description",
            "issue_links",
            "pr_links",
            "doc_links",
            "updated_at",
        ] {
            assert!(
                column_names.contains(&col.to_string()),
                "missing column: {col}"
            );
        }
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
        let id1 = create(&conn, "First task", TaskKind::General, None, &[]).unwrap();
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
    fn list_visible_returns_only_in_progress_blocked_review() {
        let conn = in_memory();
        let row_id: i64 = conn
            .query_row(
                "INSERT INTO tasks (title, status, kind, created_at) VALUES ('t','backlog','general',?1) RETURNING id",
                params![Utc::now().to_rfc3339()],
                |r| r.get(0),
            )
            .unwrap();
        for status in &["in-progress", "blocked", "review"] {
            conn.execute(
                "INSERT INTO tasks (title, status, kind, created_at) VALUES (?1, ?2, 'general', ?3)",
                params![format!("task {status}"), status, Utc::now().to_rfc3339()],
            )
            .unwrap();
        }
        let _ = row_id; // backlog row; should be excluded
        let visible = list_visible(&conn).unwrap();
        assert_eq!(visible.len(), 3);
        let statuses: Vec<String> = visible.iter().map(|t| t.status.to_string()).collect();
        assert!(statuses.contains(&"in-progress".to_string()));
        assert!(statuses.contains(&"blocked".to_string()));
        assert!(statuses.contains(&"review".to_string()));
    }

    #[test]
    fn list_visible_returns_empty_when_no_visible_tasks() {
        let conn = in_memory();
        let tasks = list_visible(&conn).unwrap();
        assert!(tasks.is_empty());
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
    fn create_with_issue_link_stores_and_returns_via_get() {
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
        assert_eq!(
            task.issue_links,
            vec!["https://github.com/owner/repo/issues/1"]
        );
    }

    #[test]
    fn create_without_optional_fields_returns_empty_defaults() {
        let conn = in_memory();
        let id = create(&conn, "Task", TaskKind::General, None, &[]).unwrap();
        let task = get(&conn, &id).unwrap();
        assert!(task.description.is_none());
        assert!(task.issue_links.is_empty());
        assert!(task.pr_links.is_empty());
        assert!(task.doc_links.is_empty());
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
        let id = create(&conn, "Task", TaskKind::General, None, &[]).unwrap();
        let before = get(&conn, &id).unwrap();
        update_status(&conn, &id, TaskStatus::Review).unwrap();
        let after = get(&conn, &id).unwrap();
        assert_eq!(after.status, TaskStatus::Review);
        assert!(after.updated_at >= before.updated_at);
    }

    #[test]
    fn update_status_errors_on_nonexistent_task() {
        let conn = in_memory();
        let id: TaskId = "TASK-9999".parse().unwrap();
        assert!(update_status(&conn, &id, TaskStatus::Review).is_err());
    }

    // ── add_comment ───────────────────────────────────────────────────────────

    #[test]
    fn add_comment_appends_in_chronological_order() {
        let conn = in_memory();
        let id = create(&conn, "Task", TaskKind::General, None, &[]).unwrap();
        add_comment(
            &conn,
            &id,
            domain::CommentAuthor::Agent,
            "Starting investigation",
        )
        .unwrap();
        add_comment(&conn, &id, domain::CommentAuthor::Agent, "Found the issue").unwrap();
        let task = get(&conn, &id).unwrap();
        assert_eq!(task.comments.len(), 2);
        assert_eq!(task.comments[0].content, "Starting investigation");
        assert_eq!(task.comments[1].content, "Found the issue");
    }

    #[test]
    fn add_comment_updates_task_updated_at() {
        let conn = in_memory();
        let id = create(&conn, "Task", TaskKind::General, None, &[]).unwrap();
        let before = get(&conn, &id).unwrap();
        add_comment(&conn, &id, domain::CommentAuthor::Agent, "Progress note").unwrap();
        let after = get(&conn, &id).unwrap();
        assert!(after.updated_at >= before.updated_at);
    }

    #[test]
    fn add_comment_errors_on_nonexistent_task() {
        let conn = in_memory();
        let id: TaskId = "TASK-9999".parse().unwrap();
        assert!(add_comment(&conn, &id, domain::CommentAuthor::Agent, "note").is_err());
    }
}
