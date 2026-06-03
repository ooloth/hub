use anyhow::Result;
use domain::{AgentTask, TaskId, TaskKind};

/// Creates a new task in `backlog` status and returns its generated `TaskId`.
pub fn create(title: &str, kind: TaskKind) -> Result<TaskId> {
    let conn = store::status::connect()?;
    store::tasks::ensure_table(&conn)?;
    store::tasks::create(&conn, title, kind)
}

/// Transitions a task from `backlog` to `ready`.
pub fn set_ready(id: &TaskId) -> Result<()> {
    let conn = store::status::connect()?;
    store::tasks::ensure_table(&conn)?;
    store::tasks::set_ready(&conn, id)
}

/// Returns all tasks that appear in the TUI unified list (in-progress, blocked, review).
pub fn list_visible() -> Result<Vec<AgentTask>> {
    let conn = store::status::connect()?;
    store::tasks::ensure_table(&conn)?;
    store::tasks::list_visible(&conn)
}
