use anyhow::{Context, Result};
use domain::{CommentAuthor, StreamBlock, Task, TaskId, TaskKind, TaskStatus};

/// Creates a new task in `backlog` status and returns its generated `TaskId`.
pub fn create(
    title: &str,
    kind: TaskKind,
    description: Option<&str>,
    links: &[String],
    repo: Option<&str>,
) -> Result<TaskId> {
    let conn = store::status_cache::connect()?;
    store::tasks::ensure_table(&conn)?;
    store::tasks::create(&conn, title, kind, description, links, repo)
}

/// Returns the full task including all fields and its comment thread.
pub fn get(id: &TaskId) -> Result<Task> {
    let conn = store::status_cache::connect()?;
    store::tasks::ensure_table(&conn)?;
    store::tasks::get(&conn, id)
}

/// Updates the status of a task and refreshes `updated_at`.
pub fn update_status(id: &TaskId, status: TaskStatus) -> Result<()> {
    let conn = store::status_cache::connect()?;
    store::tasks::ensure_table(&conn)?;
    store::tasks::update_status(&conn, id, status)
}

/// Appends a comment to a task and refreshes the task's `updated_at`.
pub fn add_comment(id: &TaskId, author: CommentAuthor, content: &str) -> Result<()> {
    let conn = store::status_cache::connect()?;
    store::tasks::ensure_table(&conn)?;
    store::task_comments::add(&conn, id, author, content)
}

/// Transitions a task from `backlog` to `ready`.
pub fn set_ready(id: &TaskId) -> Result<()> {
    let conn = store::status_cache::connect()?;
    store::tasks::ensure_table(&conn)?;
    store::tasks::set_ready(&conn, id)
}

/// Returns all tasks that appear in the TUI unified list (in-progress, blocked, review).
pub fn list_visible() -> Result<Vec<Task>> {
    let conn = store::status_cache::connect()?;
    store::tasks::ensure_table(&conn)?;
    store::tasks::list_visible(&conn)
}

/// Reads and parses the Claude Code session JSONL for `session_id` in `cwd`.
/// Returns `Ok(vec![])` if the file does not yet exist (session hasn't started writing).
pub async fn read_session_stream(cwd: &str, session_id: &str) -> Result<Vec<StreamBlock>> {
    let home = std::env::var("HOME").context("HOME env var not set")?;
    let encoded = domain::encode_project_path(cwd);
    let path = format!("{home}/.claude/projects/{encoded}/{session_id}.jsonl");
    let text = match tokio::fs::read_to_string(&path).await {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).context(format!("reading session JSONL at {path}")),
    };
    Ok(domain::parse_session_jsonl(&text))
}
