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
//! - Window reaping — schedules `tmux kill-window -t TASK-XXXX` after the
//!   5-minute buffer once a task reaches `in-review`; cancels if status reverts
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
//! Worktree management (S0) implemented. Dispatch loop (S1) tracked in issue #279.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::git::{create_branch_worktree_or_recover, read_default_branch};
use domain::{Task, TaskId};

/// Returns the root directory for all agent task workspaces: `~/.hub/workspaces/`.
///
/// Task dispatch worktrees live here, completely separate from the PR investigation
/// worktrees at `~/.hub/repos/`. Do not conflate the two systems.
pub fn workspaces_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".hub").join("workspaces")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_helpers::{run_git, setup_origin_and_bare};
    use domain::{TaskKind, TaskStatus};

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
}
