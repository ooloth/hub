use anyhow::{Context, Result};
use chrono::Utc;
use domain::{CommentAuthor, TaskComment, TaskId};
use rusqlite::{params, Connection};

/// Creates the `task_comments` table. Called from `tasks::ensure_table`.
///
/// # Errors
/// Returns an error if the SQL statement fails.
pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_comments (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            author     TEXT    NOT NULL CHECK(author IN ('human', 'agent')),
            content    TEXT    NOT NULL,
            created_at TEXT    NOT NULL
        );",
    )
    .context("failed to ensure task_comments schema")
}

/// Appends a comment to a task and refreshes the task's `updated_at`.
///
/// # Errors
/// Returns an error if the task does not exist or a SQL statement fails.
pub fn add(conn: &Connection, id: &TaskId, author: CommentAuthor, content: &str) -> Result<()> {
    let row_id = crate::tasks::task_row_id(id)?;
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
    let _ = conn
        .execute(
            "INSERT INTO task_comments (task_id, author, content, created_at)
         VALUES (?1, ?2, ?3, ?4)",
            params![row_id, author.to_string(), content, now],
        )
        .context("failed to insert task comment")?;
    let _ = conn
        .execute(
            "UPDATE tasks SET updated_at = ?1 WHERE id = ?2",
            params![now, row_id],
        )
        .context("failed to refresh task updated_at")?;
    Ok(())
}

/// Returns all comments for a task row, ordered chronologically.
pub(crate) fn for_task(conn: &Connection, task_row_id: i64) -> Result<Vec<TaskComment>> {
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
