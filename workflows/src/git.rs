//! Low-level git primitives — generic wrappers around `git` subprocess calls.
//!
//! This module contains only reusable git operations with no knowledge of hub's
//! domain concepts (investigations, projects). Feature-specific worktree
//! logic — where to put worktrees, when to clean them up, branch naming conventions —
//! belongs in the feature's own module (`fetch.rs`), not here.
// pub(crate) fns in a pub(crate) mod are effectively the same visibility; both lints
// fire depending on which keyword is used — suppress the noisier nursery one.
#![allow(clippy::redundant_pub_crate)]

use anyhow::{Context, Result};
use std::path::Path;
use tokio::process::Command;

/// Reads the default branch name from a bare repo's HEAD file.
/// Returns `None` for detached HEAD or unreadable HEAD.
pub(crate) fn read_default_branch(bare: &Path) -> Option<String> {
    let head = std::fs::read_to_string(bare.join("HEAD")).ok()?;
    let branch = head.trim().strip_prefix("ref: refs/heads/")?;
    Some(branch.to_owned())
}

/// Runs `git worktree add <worktree> <branch>` in `bare_str`, where `branch` already
/// exists. If the add fails but the worktree directory now exists, treats it as
/// success — another process won the race. Otherwise surfaces the original git error.
pub(crate) async fn add_worktree_or_recover(
    bare_str: &str,
    worktree: &Path,
    branch: &str,
) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    let add = Command::new("git")
        .args(["-C", bare_str, "worktree", "add", &worktree_str, branch])
        .output()
        .await
        .context("git worktree add invocation failed")?;
    if add.status.success() {
        return Ok(());
    }
    if worktree.is_dir() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&add.stderr);
    anyhow::bail!("git worktree add failed: {stderr}")
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use std::path::{Path, PathBuf};
    use tokio::process::Command;

    pub(crate) async fn run_git(args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .output()
            .await
            .expect("git not found");
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    pub(crate) async fn git_rev_parse(dir: &Path, refname: &str) -> String {
        let out = Command::new("git")
            .args(["-C", &dir.to_string_lossy(), "rev-parse", refname])
            .output()
            .await
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Creates an origin repo with one commit and a properly-configured bare clone
    /// that mirrors the production setup (refs/remotes/origin/* refspec).
    pub(crate) async fn setup_origin_and_bare(tmp: &Path) -> (PathBuf, PathBuf) {
        let origin = tmp.join("origin");
        let bare = tmp.join("bare");
        let origin_str = origin.to_string_lossy().into_owned();
        let bare_str = bare.to_string_lossy().into_owned();

        run_git(&["-c", "init.defaultBranch=main", "init", &origin_str]).await;
        run_git(&["-C", &origin_str, "config", "user.email", "t@t.com"]).await;
        run_git(&["-C", &origin_str, "config", "user.name", "Test"]).await;
        run_git(&["-C", &origin_str, "commit", "--allow-empty", "-m", "init"]).await;

        run_git(&["clone", "--bare", &origin_str, &bare_str]).await;
        run_git(&[
            "-C",
            &bare_str,
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ])
        .await;
        run_git(&["-C", &bare_str, "fetch", "origin"]).await;

        (origin, bare)
    }

    /// Pushes an empty commit to origin and returns its SHA.
    pub(crate) async fn push_commit(origin: &Path, message: &str) -> String {
        let origin_str = origin.to_string_lossy().into_owned();
        run_git(&["-C", &origin_str, "commit", "--allow-empty", "-m", message]).await;
        git_rev_parse(origin, "HEAD").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_helpers::*;

    // ── read_default_branch ───────────────────────────────────────────────────

    #[test]
    fn read_default_branch_extracts_branch_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("HEAD"), "ref: refs/heads/main\n").unwrap();
        assert_eq!(read_default_branch(dir.path()).as_deref(), Some("main"));
    }

    #[test]
    fn read_default_branch_returns_none_for_detached_head() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("HEAD"), "abc1234567890abcdef\n").unwrap();
        assert!(read_default_branch(dir.path()).is_none());
    }

    // ── add_worktree_or_recover ───────────────────────────────────────────────

    #[tokio::test]
    async fn add_worktree_or_recover_succeeds_on_fresh_bare() {
        let tmp = tempfile::tempdir().unwrap();
        let (origin, bare) = setup_origin_and_bare(tmp.path()).await;
        let origin_str = origin.to_string_lossy().into_owned();
        let bare_str = bare.to_string_lossy().into_owned();

        run_git(&["-C", &origin_str, "checkout", "-b", "feat/x"]).await;
        run_git(&["-C", &origin_str, "commit", "--allow-empty", "-m", "x"]).await;
        run_git(&["-C", &bare_str, "fetch", "origin"]).await;
        run_git(&[
            "-C",
            &bare_str,
            "branch",
            "pr-9",
            "refs/remotes/origin/feat/x",
        ])
        .await;

        let worktree = bare.join("pr-9");
        add_worktree_or_recover(&bare_str, &worktree, "pr-9")
            .await
            .unwrap();
        assert!(worktree.is_dir(), "worktree must be created");
    }

    #[tokio::test]
    async fn add_worktree_or_recover_returns_ok_when_directory_already_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let (_origin, bare) = setup_origin_and_bare(tmp.path()).await;
        let bare_str = bare.to_string_lossy().into_owned();

        let worktree = bare.join("pr-7");
        std::fs::create_dir(&worktree).unwrap();

        let result = add_worktree_or_recover(&bare_str, &worktree, "pr-7").await;
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[tokio::test]
    async fn add_worktree_or_recover_propagates_error_when_directory_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let (_origin, bare) = setup_origin_and_bare(tmp.path()).await;
        let bare_str = bare.to_string_lossy().into_owned();

        let worktree = bare.join("pr-nope");
        let result = add_worktree_or_recover(&bare_str, &worktree, "nonexistent-branch").await;
        assert!(result.is_err(), "expected Err, got {result:?}");
    }
}
