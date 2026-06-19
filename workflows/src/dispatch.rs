//! Task dispatch: claim the oldest ready task and spawn its agent session.
//!
//! This module is the **only place** that should call `tmux new-window` or
//! construct the `claude` command. If tmux is ever replaced or the agent runner
//! changes, this is the file to update — nothing else in the codebase should
//! need to change for those swaps.
//!
//! # Responsibilities
//!
//! - `dispatch()` — called by the TUI's 30-second tick; atomically claims the
//!   oldest `ready` task and spawns a detached named tmux window running Claude
//!   Code in the task's worktree
//! - `ensure_task_worktree()` — creates or reuses the persistent workspace at
//!   `~/.hub/workspaces/TASK-XXXX/<project>/` on branch `agent/TASK-XXXX`
//! - `clean_task_worktrees()` — deferred cleanup; removes a workspace only when
//!   all three guards pass (terminal, ≥72h stale, no unpushed commits)
//! - `reap_idle_windows()` — runs `tmux kill-window -t TASK-XXXX` for tasks that
//!   have been `in-review` longer than the buffer. Stateless: keyed off
//!   `updated_at` each poll, so a manual revert cancels the reap with no timer
//!
//! # What this module does NOT own
//!
//! - Session state detection (completion, stall, self-heal) → `agent_session`
//! - Task CRUD → `tasks` and `store::tasks`
//! - PR investigation worktrees (a completely separate system at `~/.hub/repos/`) → `fetch`
//! - Low-level git primitives → `git`
//!
//! # Design note
//!
//! The coupling to tmux (~5 lines) and to the `claude` CLI flags (~10 lines) is
//! intentional and accepted. See `docs/decisions/015-accept-tmux-and-claude-code-coupling.md`.
//!
//! # Implementation
//!
//! Worktree management (S0), dispatch loop (S1), and window reaping (S3)
//! implemented.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use store::Connection;
use tokio::process::Command;

use crate::git::{create_branch_worktree_or_recover, read_default_branch};
use domain::{Task, TaskId, TaskKind, TaskStatus};

/// Returns the system prompt for the given task kind, embedded at compile time.
const fn system_prompt_for_kind(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::Implement => include_str!("../../prompts/tasks/implement.md"),
        TaskKind::Review => include_str!("../../prompts/tasks/review.md"),
        TaskKind::Debug => include_str!("../../prompts/tasks/debug.md"),
    }
}

/// Builds the task-specific user message from the full task record.
///
/// Includes the task ID, title, kind, description, links, and comment thread,
/// followed by the exact completion commands and escalation path the agent
/// must follow. The task ID in the completion steps is the real TASK-XXXX
/// value so the agent never has to guess it.
pub(crate) fn build_task_prompt(task: &Task) -> String {
    let mut lines: Vec<String> = vec![
        format!("Task {}: {}", task.id, task.title),
        format!("Kind: {}", task.kind),
    ];

    if let Some(ref desc) = task.description {
        lines.push(String::new());
        lines.push(desc.clone());
    }

    if !task.links.is_empty() {
        lines.push(String::new());
        lines.push("Links:".into());
        for link in &task.links {
            lines.push(format!("- {link}"));
        }
    }

    if !task.comments.is_empty() {
        lines.push(String::new());
        lines.push("Comments:".into());
        for c in &task.comments {
            lines.push(format!("{} ({}): {}", c.author, c.created_at, c.content));
        }
    }

    let id = &task.id;
    lines.push(String::new());
    lines.push("---".into());
    lines.push(String::new());
    lines.push("Completion steps:".into());
    lines.push(format!(
        "1. Write a session log to ~/.hub/agent-session-logs/{id}-<slug>.md\n   \
         Sections: Summary, Changes Made, PRs Created, Gotchas, Next Steps\n   \
         (<slug> is a short kebab-case summary of the task title)"
    ));
    lines.push(format!(
        "2. hub task link {id} --value ~/.hub/agent-session-logs/{id}-<slug>.md"
    ));
    lines.push(format!("3. hub task report {id} --status in-review"));
    lines.push(String::new());
    lines.push("If blocked at any point:".into());
    lines.push(format!(
        "1. hub task comment {id} --content \"<your explanation>\""
    ));
    lines.push(format!("2. hub task report {id} --status blocked"));

    lines.join("\n")
}

/// Returns the root directory for all agent task workspaces: `~/.hub/workspaces/`.
///
/// Task dispatch worktrees live here, completely separate from the PR investigation
/// worktrees at `~/.hub/repos/`. Do not conflate the two systems.
#[must_use]
pub fn workspaces_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".hub").join("workspaces")
}

/// Returns the directory a dispatched task's agent session runs in.
///
/// This is the `cwd` passed to `claude`, and therefore the directory under which
/// Claude writes its session JSONL (`~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`).
///
/// For a task with a repo this is the project worktree
/// `~/.hub/workspaces/<task_id>/<repo_name>/`; for a repo-less task it is the
/// task workspace root `~/.hub/workspaces/<task_id>/`.
///
/// Pure: derives the path without touching the filesystem. Both dispatch (which
/// creates the directory) and the TUI session poll (which reads the stream from
/// it) call this so they agree on where the session lives.
#[must_use]
pub fn task_workspace_path(task: &Task) -> PathBuf {
    let base = workspaces_dir().join(task.id.to_string());
    match &task.repo {
        Some(slug) => base.join(slug.repo_name()),
        None => base,
    }
}

/// Creates or reuses the persistent workspace for a task.
///
/// On first call: creates `~/.hub/workspaces/<task_id>/<project>/` as a linked
/// git worktree on a new branch `agent/<task_id>` checked out from the default
/// branch of `bare` (e.g. `origin/main`). Also creates
/// `~/.hub/workspaces/<task_id>/` if it doesn't exist yet.
///
/// On re-entry (directory already exists): returns the existing path unchanged.
/// This is intentional — task worktrees survive session termination and are
/// reused on resume.
///
/// # Errors
/// Returns an error if `bare` has no readable default branch or if the
/// underlying `git worktree add` fails and the directory does not already exist.
pub async fn ensure_task_worktree(
    bare: &Path,
    task_id: &TaskId,
    project_name: &str,
) -> Result<PathBuf> {
    let worktree = workspaces_dir()
        .join(task_id.to_string())
        .join(project_name);

    if worktree.is_dir() {
        return Ok(worktree);
    }

    // Best-effort fetch so the new branch starts from a reasonably fresh trunk.
    // Failure here blocks first creation but does not block re-entry.
    let bare_str = bare.to_string_lossy().into_owned();
    let fetch = Command::new("git")
        .args(["-C", &bare_str, "fetch", "origin"])
        .output()
        .await
        .context("git fetch origin failed")?;
    if !fetch.status.success() {
        let stderr = String::from_utf8_lossy(&fetch.stderr);
        anyhow::bail!("git fetch origin failed: {stderr}");
    }

    let default_branch = read_default_branch(bare)
        .ok_or_else(|| anyhow::anyhow!("could not detect default branch in {bare_str}"))?;

    // Create the task-id parent dir so the project subdir can be added as a worktree.
    let task_dir = workspaces_dir().join(task_id.to_string());
    tokio::fs::create_dir_all(&task_dir)
        .await
        .with_context(|| format!("failed to create workspace dir {}", task_dir.display()))?;

    let branch = format!("agent/{task_id}");
    let start_point = format!("refs/remotes/origin/{default_branch}");

    create_branch_worktree_or_recover(&bare_str, &worktree, &branch, &start_point)
        .await
        .with_context(|| format!("failed to create worktree for {task_id}"))?;

    Ok(worktree)
}

/// Classifies whether a worktree directory has commits that have not been pushed.
///
/// `UpstreamAbsent` is treated the same as `Unpushed` throughout cleanup — the
/// agent has not yet pushed the branch, so we cannot verify safety and must not
/// delete. This inference is explicit in the type rather than being a silent
/// fallthrough.
#[derive(Debug, PartialEq)]
enum WorktreeCommitState {
    Clean,
    Unpushed,
    /// No upstream configured — conservative: treat as Unpushed.
    UpstreamAbsent,
}

/// Returns the `TaskId`s whose worktrees are eligible for cleanup.
///
/// A task passes the pure guards (conditions 1 and 2) when:
/// 1. Its status is terminal (`done`, `failed`, or `cancelled`)
/// 2. Its `updated_at` timestamp is ≥72 hours before `now`
///
/// Note: `updated_at` is bumped by comments as well as status changes. A comment
/// posted after a task goes terminal resets the 72-hour clock. This is acceptable
/// — the effect is conservative (deletion is delayed, never premature).
///
/// Tasks with an unparseable `updated_at` are skipped with a warning; they do not
/// block cleanup of other candidates.
///
/// Condition 3 (no unpushed commits) is I/O and is checked by the caller.
pub(crate) fn cleanup_candidates(tasks: &[Task], now: DateTime<Utc>) -> Vec<TaskId> {
    let threshold = chrono::Duration::hours(72);
    tasks
        .iter()
        .filter(|t| t.status.is_terminal())
        .filter_map(|t| {
            let updated_at = match chrono::DateTime::parse_from_rfc3339(&t.updated_at) {
                Ok(dt) => dt.with_timezone(&Utc),
                Err(e) => {
                    eprintln!(
                        "warn: skipping cleanup candidate {} — unparseable updated_at {:?}: {e}",
                        t.id, t.updated_at
                    );
                    return None;
                }
            };
            if now - updated_at >= threshold {
                Some(t.id.clone())
            } else {
                None
            }
        })
        .collect()
}

/// Removes task workspace directories that pass all three cleanup guards:
/// 1. Task is terminal (checked by `cleanup_candidates`)
/// 2. Terminal for ≥72 hours (checked by `cleanup_candidates`)
/// 3. Worktree has no unpushed commits (checked here via `git log @{u}..HEAD`)
///
/// Workspaces that do not exist on disk are silently skipped. Workspaces with
/// no upstream configured are kept (treated as having unpushed work).
///
/// # Errors
/// Returns an error if a worktree cannot be created or the session cannot be started.
pub async fn clean_task_worktrees(
    workspaces: &Path,
    tasks: &[Task],
    now: DateTime<Utc>,
) -> Result<()> {
    let candidates = cleanup_candidates(tasks, now);

    for task_id in candidates {
        let task_workspace = workspaces.join(task_id.to_string());
        if !task_workspace.is_dir() {
            continue;
        }

        // A workspace contains one project subdir per dispatched repo.
        // For S0 there is only one, but we iterate to be safe.
        let mut safe_to_remove = true;
        let mut read_dir = tokio::fs::read_dir(&task_workspace)
            .await
            .with_context(|| format!("failed to read workspace {}", task_workspace.display()))?;
        while let Some(entry) = read_dir.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                continue;
            }
            match worktree_commit_state(&entry.path()).await {
                WorktreeCommitState::Clean => {}
                WorktreeCommitState::Unpushed | WorktreeCommitState::UpstreamAbsent => {
                    safe_to_remove = false;
                    break;
                }
            }
        }

        if safe_to_remove {
            tokio::fs::remove_dir_all(&task_workspace)
                .await
                .with_context(|| {
                    format!("failed to remove workspace {}", task_workspace.display())
                })?;
        }
    }

    Ok(())
}

/// Checks whether a worktree directory has unpushed commits.
async fn worktree_commit_state(worktree: &Path) -> WorktreeCommitState {
    let worktree_str = worktree.to_string_lossy().into_owned();
    let out = Command::new("git")
        .args(["-C", &worktree_str, "log", "@{u}..HEAD", "--oneline"])
        .output()
        .await;
    match out {
        Err(_) => WorktreeCommitState::UpstreamAbsent,
        Ok(o) if !o.status.success() => {
            // git exits non-zero when no upstream is configured.
            WorktreeCommitState::UpstreamAbsent
        }
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.trim().is_empty() {
                WorktreeCommitState::Clean
            } else {
                WorktreeCommitState::Unpushed
            }
        }
    }
}

/// How long a task sits in `in-review` before its idle tmux window is reaped.
/// The session stays resumable via `claude --resume`, so the window itself is
/// disposable; the buffer just leaves time for a quick manual correction.
const REAP_BUFFER_MINS: i64 = 5;

/// Returns the `TaskId`s whose idle tmux window is eligible for reaping: status
/// is `in-review` and `updated_at` is at least `buffer` before `now`.
///
/// Cancellation is implicit: moving a task out of `in-review` updates its status
/// and `updated_at`, so it stops appearing here — no timer to cancel. Tasks with
/// an unparseable `updated_at` are skipped with a warning, never reaped blindly.
pub(crate) fn reap_candidates(
    tasks: &[Task],
    now: DateTime<Utc>,
    buffer: chrono::Duration,
) -> Vec<TaskId> {
    tasks
        .iter()
        .filter(|t| t.status == TaskStatus::InReview)
        .filter_map(|t| {
            let updated_at = match chrono::DateTime::parse_from_rfc3339(&t.updated_at) {
                Ok(dt) => dt.with_timezone(&Utc),
                Err(e) => {
                    eprintln!(
                        "warn: skipping reap candidate {} — unparseable updated_at {:?}: {e}",
                        t.id, t.updated_at
                    );
                    return None;
                }
            };
            if now - updated_at >= buffer {
                Some(t.id.clone())
            } else {
                None
            }
        })
        .collect()
}

/// Kills the idle tmux window of every task that has been `in-review` longer than [`REAP_BUFFER_MINS`].
///
/// Best-effort: a kill that fails is logged, never propagated, so one bad window
/// can't block the poll. Only kills if the window still exists.
///
/// The existence check keeps this idempotent — once a window is gone, later polls
/// skip it instead of erroring on every tick.
///
/// # Errors
/// Returns an error if a git or filesystem operation fails.
pub async fn reap_idle_windows(tasks: &[Task], now: DateTime<Utc>) -> Result<()> {
    let candidates = reap_candidates(tasks, now, chrono::Duration::minutes(REAP_BUFFER_MINS));
    if candidates.is_empty() {
        return Ok(());
    }

    let existing = existing_window_names().await?;
    for id in candidates {
        let name = id.to_string();
        if !existing.contains(&name) {
            continue;
        }
        match Command::new("tmux")
            .args(["kill-window", "-t", &name])
            .output()
            .await
        {
            Ok(o) if o.status.success() => {}
            Ok(o) => eprintln!(
                "warn: tmux kill-window {name} failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ),
            Err(e) => eprintln!("warn: tmux kill-window {name}: {e}"),
        }
    }
    Ok(())
}

/// Returns the names of all tmux windows across every session. An absent or
/// stopped tmux server yields an empty list rather than an error — there is
/// simply nothing to reap.
async fn existing_window_names() -> Result<Vec<String>> {
    let out = Command::new("tmux")
        .args(["list-windows", "-a", "-F", "#{window_name}"])
        .output()
        .await
        .context("tmux list-windows failed")?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(std::string::ToString::to_string)
        .collect())
}

/// Maximum number of concurrent in-progress agent sessions.
const DISPATCH_CAP: u32 = 1;

/// Claims the oldest `ready` task and spawns a detached Claude Code session in a
/// named tmux window. Called by the TUI's 30-second dispatch tick.
///
/// Returns `Ok(())` without spawning if the in-progress count is at the cap, if
/// no tasks are ready, or if a concurrent tick claimed the task first.
///
/// # Errors
/// Returns an error if a git or filesystem operation fails.
#[allow(clippy::future_not_send)] // rusqlite Connection is not Sync by design
pub async fn dispatch() -> Result<()> {
    let conn = store::status_cache::connect()?;
    store::tasks::ensure_table(&conn)?;
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let claude_json = std::path::PathBuf::from(home).join(".claude.json");
    dispatch_inner(&conn, &crate::fetch::repos_dir(), &claude_json).await
}

#[allow(clippy::future_not_send)] // rusqlite Connection is not Sync by design
async fn dispatch_inner(conn: &Connection, repos_dir: &Path, claude_json: &Path) -> Result<()> {
    if store::tasks::count_in_progress(conn)? >= DISPATCH_CAP {
        return Ok(());
    }

    let Some(ready) = store::tasks::oldest_ready(conn)? else {
        return Ok(());
    };

    let session_id = ready.id.session_id();
    let claimed = store::tasks::claim_for_dispatch(conn, &ready.id, &session_id.to_string())?;
    if !claimed {
        return Ok(());
    }

    // Fetch the full task after claiming so we can build the system and task
    // prompts. The ReadyTask returned by oldest_ready only carries id/repo/kind.
    let task = store::tasks::get(conn, &ready.id)
        .with_context(|| format!("failed to read task {} after claim", ready.id))?;

    let workspace = if let Some(ref slug) = task.repo {
        let bare = repos_dir.join(slug.repo_name());
        ensure_task_worktree(&bare, &task.id, slug.repo_name()).await?
    } else {
        let dir = task_workspace_path(&task);
        tokio::fs::create_dir_all(&dir)
            .await
            .with_context(|| format!("failed to create workspace dir for {}", task.id))?;
        dir
    };

    // Pre-trust the workspace so the session starts without the safety prompt.
    // Soft failure: on error, warn and proceed — stall detection (#281) is the
    // safety net if the trust dialog blocks the session.
    if let Err(e) = crate::claude_trust::trust_workspace(claude_json, &workspace) {
        eprintln!(
            "warn: could not pre-trust workspace {}: {e}",
            workspace.display()
        );
    }

    let system_prompt = system_prompt_for_kind(task.kind);
    let task_prompt = build_task_prompt(&task);
    let window_name = task.id.to_string();
    let workspace_str = workspace.to_string_lossy().into_owned();
    let claude_cmd = format!(
        "claude --session-id {session_id} --dangerously-skip-permissions --model {} \
         --append-system-prompt \"$HUB_SYSTEM_PROMPT\" \"$HUB_TASK_PROMPT\"",
        task.kind.model()
    );

    let out = Command::new("tmux")
        .args([
            "new-window",
            "-d",
            "-n",
            &window_name,
            "-c",
            &workspace_str,
            "-e",
            &format!("HUB_SYSTEM_PROMPT={system_prompt}"),
            "-e",
            &format!("HUB_TASK_PROMPT={task_prompt}"),
            &claude_cmd,
        ])
        .output()
        .await
        .with_context(|| format!("failed to spawn tmux window for {}", task.id))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("tmux new-window failed for {}: {stderr}", task.id);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_helpers::{run_git, setup_origin_and_bare};
    use domain::{TaskKind, TaskStatus};

    fn in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        store::tasks::ensure_table(&conn).unwrap();
        conn
    }

    // ── dispatch_inner ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn dispatch_inner_skips_when_no_ready_tasks() {
        let conn = in_memory();
        let repos_dir = tempfile::tempdir().unwrap();
        let claude_json = repos_dir.path().join(".claude.json");
        let result = dispatch_inner(&conn, repos_dir.path(), &claude_json).await;
        assert!(
            result.is_ok(),
            "expected Ok when no ready tasks, got {result:?}"
        );
        assert_eq!(store::tasks::count_in_progress(&conn).unwrap(), 0);
    }

    #[tokio::test]
    async fn dispatch_inner_skips_when_in_progress_at_cap() {
        let conn = in_memory();
        let repos_dir = tempfile::tempdir().unwrap();
        // Manually insert an in-progress task to hit the cap.
        let _ = conn.execute(
            "INSERT INTO tasks (title, status, kind, created_at) VALUES ('running', 'in-progress', 'implement', '2024-01-01T00:00:00Z')",
            [],
        ).unwrap();
        // Add a ready task that should NOT be claimed.
        let _ = store::tasks::create(
            &conn,
            "waiting",
            TaskKind::Implement,
            None,
            &[],
            None,
            &domain::TaskOrigin::Idea,
        )
        .unwrap();
        let _ = conn
            .execute(
                "UPDATE tasks SET status = 'ready' WHERE title = 'waiting'",
                [],
            )
            .unwrap();

        let claude_json = repos_dir.path().join(".claude.json");
        let result = dispatch_inner(&conn, repos_dir.path(), &claude_json).await;
        assert!(result.is_ok());
        // The waiting task must still be ready, not in-progress.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status = 'ready'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "ready task must remain unclaimed when cap is reached"
        );
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    fn make_task(id: &str, status: TaskStatus, updated_at: DateTime<Utc>) -> Task {
        use chrono::Duration;
        use domain::Urgency;
        Task {
            id: id.parse().unwrap(),
            title: format!("Task {id}"),
            description: None,
            status,
            kind: TaskKind::Implement,
            session_id: None,
            repo: None,
            origin: domain::TaskOrigin::Idea,
            links: vec![],
            created_at: updated_at.to_rfc3339(),
            updated_at: updated_at.to_rfc3339(),
            age: Duration::zero(),
            urgency: Urgency::Low,
            comments: vec![],
        }
    }

    fn hours_ago(h: i64) -> DateTime<Utc> {
        Utc::now() - chrono::Duration::hours(h)
    }

    // ── cleanup_candidates (pure unit tests) ─────────────────────────────────

    #[test]
    fn cleanup_candidates_empty_when_no_tasks() {
        let result = cleanup_candidates(&[], Utc::now());
        assert!(result.is_empty());
    }

    #[test]
    fn cleanup_candidates_excludes_non_terminal_statuses() {
        let now = Utc::now();
        let long_ago = now - chrono::Duration::hours(200);
        let tasks = vec![
            make_task("TASK-0001", TaskStatus::Backlog, long_ago),
            make_task("TASK-0002", TaskStatus::Ready, long_ago),
            make_task("TASK-0003", TaskStatus::InProgress, long_ago),
            make_task("TASK-0004", TaskStatus::Blocked, long_ago),
            make_task("TASK-0005", TaskStatus::InReview, long_ago),
        ];
        assert!(cleanup_candidates(&tasks, now).is_empty());
    }

    #[test]
    fn cleanup_candidates_excludes_terminal_under_72h() {
        let now = Utc::now();
        let tasks = vec![
            make_task("TASK-0001", TaskStatus::Done, hours_ago(71)),
            make_task("TASK-0002", TaskStatus::Failed, hours_ago(0)),
            make_task("TASK-0003", TaskStatus::Cancelled, hours_ago(71)),
        ];
        assert!(cleanup_candidates(&tasks, now).is_empty());
    }

    #[test]
    fn cleanup_candidates_includes_terminal_at_exactly_72h() {
        let now = Utc::now();
        let exactly_72h_ago = now - chrono::Duration::hours(72);
        let tasks = vec![make_task("TASK-0001", TaskStatus::Done, exactly_72h_ago)];
        let result = cleanup_candidates(&tasks, now);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].to_string(), "TASK-0001");
    }

    #[test]
    fn cleanup_candidates_includes_terminal_over_72h() {
        let now = Utc::now();
        let tasks = vec![
            make_task("TASK-0001", TaskStatus::Done, hours_ago(100)),
            make_task("TASK-0002", TaskStatus::Failed, hours_ago(73)),
            make_task("TASK-0003", TaskStatus::Cancelled, hours_ago(200)),
        ];
        let result = cleanup_candidates(&tasks, now);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn cleanup_candidates_skips_task_with_unparseable_updated_at() {
        let now = Utc::now();
        // Construct a task with a bad updated_at by direct struct construction.
        use chrono::Duration;
        use domain::Urgency;
        let bad_task = Task {
            id: "TASK-0001".parse().unwrap(),
            title: "bad".into(),
            description: None,
            status: TaskStatus::Done,
            kind: TaskKind::Implement,
            session_id: None,
            repo: None,
            origin: domain::TaskOrigin::Idea,
            links: vec![],
            created_at: hours_ago(100).to_rfc3339(),
            updated_at: "not-a-date".into(),
            age: Duration::zero(),
            urgency: Urgency::Low,
            comments: vec![],
        };
        let result = cleanup_candidates(&[bad_task], now);
        assert!(
            result.is_empty(),
            "task with bad updated_at must be skipped"
        );
    }

    #[test]
    fn cleanup_candidates_mixes_eligible_and_ineligible() {
        let now = Utc::now();
        let tasks = vec![
            make_task("TASK-0001", TaskStatus::Done, hours_ago(100)), // eligible
            make_task("TASK-0002", TaskStatus::InProgress, hours_ago(200)), // not terminal
            make_task("TASK-0003", TaskStatus::Cancelled, hours_ago(10)), // terminal but <72h
            make_task("TASK-0004", TaskStatus::Failed, hours_ago(80)), // eligible
        ];
        let result = cleanup_candidates(&tasks, now);
        let ids: Vec<String> = result.iter().map(|id| id.to_string()).collect();
        assert_eq!(ids, vec!["TASK-0001", "TASK-0004"]);
    }

    // ── reap_candidates (pure unit tests) ────────────────────────────────────

    fn mins_ago(m: i64) -> DateTime<Utc> {
        Utc::now() - chrono::Duration::minutes(m)
    }

    #[test]
    fn reap_candidates_includes_in_review_past_buffer() {
        let now = Utc::now();
        let tasks = vec![make_task("TASK-0001", TaskStatus::InReview, mins_ago(6))];
        let result = reap_candidates(&tasks, now, chrono::Duration::minutes(5));
        let ids: Vec<String> = result.iter().map(|id| id.to_string()).collect();
        assert_eq!(ids, vec!["TASK-0001"]);
    }

    #[test]
    fn reap_candidates_excludes_in_review_under_buffer() {
        let now = Utc::now();
        let tasks = vec![make_task("TASK-0001", TaskStatus::InReview, mins_ago(4))];
        assert!(reap_candidates(&tasks, now, chrono::Duration::minutes(5)).is_empty());
    }

    #[test]
    fn reap_candidates_includes_in_review_at_exactly_buffer() {
        let now = Utc::now();
        let exactly = now - chrono::Duration::minutes(5);
        let tasks = vec![make_task("TASK-0001", TaskStatus::InReview, exactly)];
        let result = reap_candidates(&tasks, now, chrono::Duration::minutes(5));
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn reap_candidates_excludes_non_in_review_statuses() {
        let now = Utc::now();
        let old = mins_ago(60);
        let tasks = vec![
            make_task("TASK-0001", TaskStatus::InProgress, old),
            make_task("TASK-0002", TaskStatus::Blocked, old),
            make_task("TASK-0003", TaskStatus::Done, old),
            make_task("TASK-0004", TaskStatus::Backlog, old),
        ];
        assert!(reap_candidates(&tasks, now, chrono::Duration::minutes(5)).is_empty());
    }

    #[test]
    fn reap_candidates_skips_task_with_unparseable_updated_at() {
        use chrono::Duration;
        use domain::Urgency;
        let bad = Task {
            id: "TASK-0001".parse().unwrap(),
            title: "bad".into(),
            description: None,
            status: TaskStatus::InReview,
            kind: TaskKind::Implement,
            session_id: None,
            repo: None,
            origin: domain::TaskOrigin::Idea,
            links: vec![],
            created_at: mins_ago(60).to_rfc3339(),
            updated_at: "not-a-date".into(),
            age: Duration::zero(),
            urgency: Urgency::Low,
            comments: vec![],
        };
        assert!(reap_candidates(&[bad], Utc::now(), chrono::Duration::minutes(5)).is_empty());
    }

    // ── task_workspace_path (pure unit tests) ────────────────────────────────

    #[test]
    fn task_workspace_path_uses_project_subdir_for_repo_task() {
        let mut task = make_task("TASK-0001", TaskStatus::InProgress, Utc::now());
        task.repo = Some(domain::RepoSlug::new("ooloth", "hub"));
        assert_eq!(
            task_workspace_path(&task),
            workspaces_dir().join("TASK-0001").join("hub"),
            "repo task runs in the project subdir of its workspace"
        );
    }

    #[test]
    fn task_workspace_path_uses_task_root_for_repoless_task() {
        let task = make_task("TASK-0002", TaskStatus::InProgress, Utc::now());
        assert_eq!(
            task_workspace_path(&task),
            workspaces_dir().join("TASK-0002"),
            "repo-less task runs in its workspace root"
        );
    }

    // ── ensure_task_worktree (integration) ───────────────────────────────────

    #[tokio::test]
    async fn ensure_task_worktree_creates_directory_on_expected_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let (_origin, bare) = setup_origin_and_bare(tmp.path()).await;

        // Override workspaces_dir by placing the workspace inside tmp.
        let workspaces = tmp.path().join("workspaces");
        tokio::fs::create_dir_all(&workspaces).await.unwrap();

        let task_id: TaskId = "TASK-0001".parse().unwrap();
        let worktree = workspaces.join("TASK-0001").join("hub");

        // Invoke with a temporary workspaces root by building the path manually
        // (ensure_task_worktree uses workspaces_dir() internally, so we verify
        // the git ops through a directly-constructed call).
        let bare_str = bare.to_string_lossy().into_owned();
        let default_branch = read_default_branch(&bare).unwrap();
        let start_point = format!("refs/remotes/origin/{default_branch}");
        let parent = workspaces.join("TASK-0001");
        tokio::fs::create_dir_all(&parent).await.unwrap();
        create_branch_worktree_or_recover(&bare_str, &worktree, "agent/TASK-0001", &start_point)
            .await
            .unwrap();

        assert!(worktree.is_dir(), "worktree directory must exist");

        // The branch agent/TASK-0001 must exist in the bare repo.
        let out = Command::new("git")
            .args(["-C", &bare_str, "rev-parse", "--verify", "agent/TASK-0001"])
            .output()
            .await
            .unwrap();
        assert!(out.status.success(), "branch agent/TASK-0001 must exist");

        drop(task_id); // satisfy compiler
    }

    #[tokio::test]
    async fn ensure_task_worktree_is_idempotent_on_reentry() {
        let tmp = tempfile::tempdir().unwrap();
        let (_origin, bare) = setup_origin_and_bare(tmp.path()).await;
        let bare_str = bare.to_string_lossy().into_owned();

        let default_branch = read_default_branch(&bare).unwrap();
        let start_point = format!("refs/remotes/origin/{default_branch}");
        let worktree = tmp.path().join("ws").join("TASK-0002").join("hub");
        tokio::fs::create_dir_all(worktree.parent().unwrap())
            .await
            .unwrap();

        // First creation.
        create_branch_worktree_or_recover(&bare_str, &worktree, "agent/TASK-0002", &start_point)
            .await
            .unwrap();
        assert!(worktree.is_dir());

        // Second call — must succeed and not create a second branch.
        create_branch_worktree_or_recover(&bare_str, &worktree, "agent/TASK-0002", &start_point)
            .await
            .unwrap();

        // Only one worktree for this branch must exist.
        let list = Command::new("git")
            .args(["-C", &bare_str, "worktree", "list", "--porcelain"])
            .output()
            .await
            .unwrap();
        let stdout = String::from_utf8_lossy(&list.stdout);
        let branch_count = stdout.matches("branch refs/heads/agent/TASK-0002").count();
        assert_eq!(branch_count, 1, "only one worktree for agent/TASK-0002");
    }

    // ── clean_task_worktrees (integration) ───────────────────────────────────

    #[tokio::test]
    async fn clean_task_worktrees_preserves_worktree_with_unpushed_commits() {
        let tmp = tempfile::tempdir().unwrap();
        let (_origin, bare) = setup_origin_and_bare(tmp.path()).await;
        let bare_str = bare.to_string_lossy().into_owned();
        let workspaces = tmp.path().join("workspaces");

        let default_branch = read_default_branch(&bare).unwrap();
        let start_point = format!("refs/remotes/origin/{default_branch}");
        let worktree = workspaces.join("TASK-0001").join("hub");
        tokio::fs::create_dir_all(worktree.parent().unwrap())
            .await
            .unwrap();
        create_branch_worktree_or_recover(&bare_str, &worktree, "agent/TASK-0001", &start_point)
            .await
            .unwrap();

        // Make a commit inside the worktree (no push → no upstream).
        run_git(&[
            "-C",
            &worktree.to_string_lossy(),
            "config",
            "user.email",
            "t@t.com",
        ])
        .await;
        run_git(&[
            "-C",
            &worktree.to_string_lossy(),
            "config",
            "user.name",
            "Test",
        ])
        .await;
        run_git(&[
            "-C",
            &worktree.to_string_lossy(),
            "commit",
            "--allow-empty",
            "-m",
            "agent work",
        ])
        .await;

        let now = Utc::now();
        let tasks = vec![make_task("TASK-0001", TaskStatus::Done, hours_ago(100))];
        clean_task_worktrees(&workspaces, &tasks, now)
            .await
            .unwrap();

        assert!(
            workspaces.join("TASK-0001").is_dir(),
            "workspace with unpushed commits must be preserved"
        );
    }

    #[tokio::test]
    async fn clean_task_worktrees_removes_workspace_when_all_conditions_met() {
        let tmp = tempfile::tempdir().unwrap();
        let (_origin, bare) = setup_origin_and_bare(tmp.path()).await;
        let bare_str = bare.to_string_lossy().into_owned();
        let workspaces = tmp.path().join("workspaces");

        let default_branch = read_default_branch(&bare).unwrap();
        let start_point = format!("refs/remotes/origin/{default_branch}");
        let worktree = workspaces.join("TASK-0002").join("hub");
        tokio::fs::create_dir_all(worktree.parent().unwrap())
            .await
            .unwrap();
        create_branch_worktree_or_recover(&bare_str, &worktree, "agent/TASK-0002", &start_point)
            .await
            .unwrap();

        // No commits in the worktree — `git log @{u}..HEAD` will fail (no upstream),
        // which means UpstreamAbsent. We need the worktree to have an upstream and be
        // clean. Push the branch so an upstream exists.
        run_git(&[
            "-C",
            &worktree.to_string_lossy(),
            "config",
            "user.email",
            "t@t.com",
        ])
        .await;
        run_git(&[
            "-C",
            &worktree.to_string_lossy(),
            "config",
            "user.name",
            "Test",
        ])
        .await;
        run_git(&[
            "-C",
            &worktree.to_string_lossy(),
            "push",
            "--set-upstream",
            "origin",
            "agent/TASK-0002",
        ])
        .await;

        let now = Utc::now();
        let tasks = vec![make_task("TASK-0002", TaskStatus::Done, hours_ago(100))];
        clean_task_worktrees(&workspaces, &tasks, now)
            .await
            .unwrap();

        assert!(
            !workspaces.join("TASK-0002").is_dir(),
            "workspace must be removed when all three guards pass"
        );
    }

    #[tokio::test]
    async fn clean_task_worktrees_preserves_workspace_terminal_under_72h() {
        let tmp = tempfile::tempdir().unwrap();
        let (_origin, bare) = setup_origin_and_bare(tmp.path()).await;
        let bare_str = bare.to_string_lossy().into_owned();
        let workspaces = tmp.path().join("workspaces");

        let default_branch = read_default_branch(&bare).unwrap();
        let start_point = format!("refs/remotes/origin/{default_branch}");
        let worktree = workspaces.join("TASK-0003").join("hub");
        tokio::fs::create_dir_all(worktree.parent().unwrap())
            .await
            .unwrap();
        create_branch_worktree_or_recover(&bare_str, &worktree, "agent/TASK-0003", &start_point)
            .await
            .unwrap();

        let now = Utc::now();
        // Terminal but only 10h ago — under the 72h threshold.
        let tasks = vec![make_task("TASK-0003", TaskStatus::Done, hours_ago(10))];
        clean_task_worktrees(&workspaces, &tasks, now)
            .await
            .unwrap();

        assert!(
            workspaces.join("TASK-0003").is_dir(),
            "workspace must be preserved when task has been terminal < 72h"
        );
    }

    #[tokio::test]
    async fn clean_task_worktrees_preserves_workspace_when_task_back_to_active() {
        let tmp = tempfile::tempdir().unwrap();
        let (_origin, bare) = setup_origin_and_bare(tmp.path()).await;
        let bare_str = bare.to_string_lossy().into_owned();
        let workspaces = tmp.path().join("workspaces");

        let default_branch = read_default_branch(&bare).unwrap();
        let start_point = format!("refs/remotes/origin/{default_branch}");
        let worktree = workspaces.join("TASK-0004").join("hub");
        tokio::fs::create_dir_all(worktree.parent().unwrap())
            .await
            .unwrap();
        create_branch_worktree_or_recover(&bare_str, &worktree, "agent/TASK-0004", &start_point)
            .await
            .unwrap();

        let now = Utc::now();
        // Task was terminal 100h ago but has since been moved back to in-progress.
        let tasks = vec![make_task(
            "TASK-0004",
            TaskStatus::InProgress,
            hours_ago(100),
        )];
        clean_task_worktrees(&workspaces, &tasks, now)
            .await
            .unwrap();

        assert!(
            workspaces.join("TASK-0004").is_dir(),
            "workspace must be preserved when task is back to active status"
        );
    }

    // ── system_prompt_for_kind ────────────────────────────────────────────────

    #[test]
    fn system_prompt_for_kind_implement_is_non_empty_with_autonomy_notice() {
        let prompt = system_prompt_for_kind(TaskKind::Implement);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("## Autonomy notice"));
    }

    #[test]
    fn system_prompt_for_kind_review_is_non_empty_with_autonomy_notice() {
        let prompt = system_prompt_for_kind(TaskKind::Review);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("## Autonomy notice"));
    }

    #[test]
    fn system_prompt_for_kind_debug_is_non_empty_with_autonomy_notice() {
        let prompt = system_prompt_for_kind(TaskKind::Debug);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("## Autonomy notice"));
    }

    // ── build_task_prompt ─────────────────────────────────────────────────────

    fn make_minimal_task(id: &str, title: &str, kind: TaskKind) -> Task {
        use chrono::Duration;
        use domain::Urgency;
        Task {
            id: id.parse().unwrap(),
            title: title.into(),
            description: None,
            status: domain::TaskStatus::InProgress,
            kind,
            session_id: None,
            repo: None,
            origin: domain::TaskOrigin::Idea,
            links: vec![],
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            age: Duration::zero(),
            urgency: Urgency::Low,
            comments: vec![],
        }
    }

    #[test]
    fn build_task_prompt_includes_task_id_and_title() {
        let task = make_minimal_task("TASK-0042", "Fix OAuth refresh", TaskKind::Implement);
        let prompt = build_task_prompt(&task);
        assert!(
            prompt.contains("TASK-0042"),
            "task ID must appear in prompt"
        );
        assert!(
            prompt.contains("Fix OAuth refresh"),
            "title must appear in prompt"
        );
    }

    #[test]
    fn build_task_prompt_includes_description_when_present() {
        let mut task = make_minimal_task("TASK-0001", "Task", TaskKind::Debug);
        task.description = Some("The token expires silently.".into());
        let prompt = build_task_prompt(&task);
        assert!(prompt.contains("The token expires silently."));
    }

    #[test]
    fn build_task_prompt_omits_description_section_when_absent() {
        let task = make_minimal_task("TASK-0001", "Task", TaskKind::Debug);
        let prompt = build_task_prompt(&task);
        assert!(
            !prompt.contains("None"),
            "no 'None' string should appear when description is absent"
        );
    }

    #[test]
    fn build_task_prompt_includes_links_when_present() {
        let mut task = make_minimal_task("TASK-0001", "Task", TaskKind::Review);
        task.links = vec!["https://github.com/ooloth/hub/pull/42".into()];
        let prompt = build_task_prompt(&task);
        assert!(prompt.contains("https://github.com/ooloth/hub/pull/42"));
    }

    #[test]
    fn build_task_prompt_includes_task_id_in_completion_steps() {
        let task = make_minimal_task("TASK-0099", "Task", TaskKind::Implement);
        let prompt = build_task_prompt(&task);
        // The completion steps must reference the real task ID so the agent
        // never has to guess it.
        let completion_section = prompt.split("---").nth(1).unwrap_or("");
        assert!(
            completion_section.contains("TASK-0099"),
            "task ID must appear in completion steps"
        );
        assert!(
            completion_section.contains("hub task report TASK-0099 --status in-review"),
            "completion steps must include the full report command"
        );
        // The link command requires the --value flag; without it the CLI
        // rejects a bare path argument, so the injected instruction must match.
        assert!(
            completion_section.contains(
                "hub task link TASK-0099 --value ~/.hub/agent-session-logs/TASK-0099-<slug>.md"
            ),
            "completion steps must use the --value flag for the link command"
        );
    }
}
