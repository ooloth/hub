use anyhow::{Context, Result};
use chrono::Utc;
use domain::{AgentTask, TaskId, TaskKind, TaskStatus};
use rusqlite::{params, Connection};

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tasks (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL,
            status      TEXT NOT NULL DEFAULT 'backlog'
                        CHECK(status IN ('backlog','ready','in-progress','blocked','review','done','archived')),
            kind        TEXT NOT NULL DEFAULT 'general'
                        CHECK(kind IN ('implement','debug','general')),
            session_id  TEXT,
            created_at  TEXT NOT NULL
        );",
    )
    .context("failed to create tasks table")
}

/// Inserts a new task in `backlog` status and returns its generated `TaskId`.
pub fn create(conn: &Connection, title: &str, kind: TaskKind) -> Result<TaskId> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO tasks (title, kind, created_at) VALUES (?1, ?2, ?3)",
        params![title, kind.to_string(), now],
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
pub fn list_visible(conn: &Connection) -> Result<Vec<AgentTask>> {
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
                Ok(AgentTask {
                    id: task_id,
                    title,
                    status,
                    kind,
                    session_id,
                    age,
                    urgency,
                })
            },
        )
        .collect()
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
    fn create_task_returns_key_with_task_prefix_format() {
        let conn = in_memory();
        let id = create(&conn, "Fix auth bug", TaskKind::Implement).unwrap();
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
        let id1 = create(&conn, "First task", TaskKind::General).unwrap();
        let id2 = create(&conn, "Second task", TaskKind::Debug).unwrap();
        assert_eq!(id1.to_string(), "TASK-0001");
        assert_eq!(id2.to_string(), "TASK-0002");
    }

    #[test]
    fn set_ready_transitions_status_from_backlog() {
        let conn = in_memory();
        let id = create(&conn, "Fix auth bug", TaskKind::Implement).unwrap();
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
        let id = create(&conn, "Fix auth bug", TaskKind::Implement).unwrap();
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
}
