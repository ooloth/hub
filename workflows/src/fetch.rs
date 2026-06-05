//! PR investigation worktrees — fetch, create, and clean up worktrees for
//! human-initiated PR review and fix sessions.
//!
//! # Two worktree systems — do not conflate
//!
//! Hub has two distinct worktree systems with different locations, branch naming,
//! lifetimes, and cleanup logic. See `docs/architecture/worktrees.md`.
//!
//! | System | Location | Owned by |
//! |---|---|---|
//! | **PR investigation** (this file) | `~/.hub/repos/<project>/pr-<N>/` | `fetch.rs` |
//! | **Task dispatch** (not yet built) | `~/.hub/workspaces/TASK-XXXX/<project>/` | `dispatch.rs` (planned) |
//!
//! PR investigation worktrees are ephemeral: created on demand for a human
//! session and cleaned up when the remote head branch is deleted.
//! Task worktrees are persistent: they survive session termination and are only
//! removed after 72 hours in a terminal state with no unpushed commits.

use anyhow::{Context, Result};
use domain::RepoSlug;
use std::{path::Path, path::PathBuf};
use tokio::process::Command;

use crate::git::{add_worktree_or_recover, read_default_branch};

pub fn repos_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".hub").join("repos")
}

/// Returns the path to the default-branch worktree for `name` if it exists.
pub fn default_branch_worktree(repos_dir: &Path, name: &str) -> Option<PathBuf> {
    let bare = repos_dir.join(name);
    let branch = read_default_branch(&bare)?;
    let worktree = bare.join(branch);
    worktree.is_dir().then_some(worktree)
}

/// Creates or updates a bare clone for each project under `~/.hub/repos/<name>/`.
///
/// On each call, fetches latest refs and prunes deleted remote branches. After
/// fetching, removes any linked worktrees whose remote tracking ref is gone
/// (indicating a merged or closed PR branch). Logs a warning to stderr for any
/// clone directory present in `~/.hub/repos/` but absent from `projects`.
///
/// # Errors
/// Returns an error if any git operation fails.
pub async fn run(projects: &[(String, String)]) -> Result<()> {
    let dir = repos_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .context("failed to create ~/.hub/repos")?;

    for (name, repo) in projects {
        fetch_project(name, repo, &dir).await?;
    }

    Ok(())
}

fn bare_clone_path(repos_dir: &Path, repo: &str) -> Result<PathBuf> {
    let slug: RepoSlug = repo.parse().map_err(|e: String| anyhow::anyhow!("{e}"))?;
    Ok(repos_dir.join(slug.repo_name()))
}

async fn fetch_project(name: &str, repo: &str, repos_dir: &Path) -> Result<()> {
    let dir = bare_clone_path(repos_dir, repo)
        .with_context(|| format!("invalid repo for project {name:?}"))?;
    let dir_str = dir.to_string_lossy().into_owned();

    if dir.exists() {
        let out = Command::new("git")
            .args(["-C", &dir_str, "fetch", "--prune", "origin"])
            .output()
            .await
            .with_context(|| format!("git fetch failed for {name}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("git fetch failed for {name}: {stderr}");
        }
        clean_merged_worktrees(&dir_str).await?;
        ensure_default_branch_worktree(&dir).await?;
    } else {
        let ssh_url = format!("git@github.com:{repo}.git");
        let out = Command::new("git")
            .args(["clone", "--bare", &ssh_url, &dir_str])
            .output()
            .await
            .with_context(|| format!("git clone failed for {name}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("git clone failed for {name}: {stderr}");
        }
        // Reconfigure the refspec so subsequent fetches go into refs/remotes/origin/*
        // rather than refs/heads/*. This avoids git refusing to update a branch that
        // is checked out in the default-branch worktree.
        let cfg = Command::new("git")
            .args([
                "-C",
                &dir_str,
                "config",
                "remote.origin.fetch",
                "+refs/heads/*:refs/remotes/origin/*",
            ])
            .output()
            .await
            .with_context(|| format!("git config refspec failed for {name}"))?;
        if !cfg.status.success() {
            let stderr = String::from_utf8_lossy(&cfg.stderr);
            anyhow::bail!("git config refspec failed for {name}: {stderr}");
        }
        // Populate refs/remotes/origin/* with the new refspec.
        let fetch = Command::new("git")
            .args(["-C", &dir_str, "fetch", "origin"])
            .output()
            .await
            .with_context(|| format!("initial git fetch failed for {name}"))?;
        if !fetch.status.success() {
            let stderr = String::from_utf8_lossy(&fetch.stderr);
            anyhow::bail!("initial git fetch failed for {name}: {stderr}");
        }
        ensure_default_branch_worktree(&dir).await?;
    }

    Ok(())
}

/// Creates a linked worktree for a PR at `<bare>/pr-<number>/` and returns its path.
///
/// Always fetches all remote tracking refs first so the agent sees the current
/// state of the base branch (e.g. origin/main). On re-entry this fetch is
/// best-effort — a network failure does not block access to an existing worktree.
/// On first creation the fetch is required; it is followed by a PR-specific fetch
/// (`pull/<number>/head`), a `git worktree add`, and setting the upstream tracking
/// branch so `git push` targets the real PR branch, not a phantom `origin/pr-<number>`.
pub async fn ensure_pr_worktree(bare: &Path, number: u64, head_branch: &str) -> Result<PathBuf> {
    let bare_str = bare.to_string_lossy().into_owned();
    let branch = format!("pr-{number}");
    let worktree = bare.join(&branch);

    let worktree_exists = worktree.is_dir();

    // Update all remote tracking refs. Non-fatal on re-entry so a network hiccup
    // does not block access to an already-created worktree.
    let fetch_all_ok = Command::new("git")
        .args(["-C", &bare_str, "fetch", "origin"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    if worktree_exists {
        return Ok(worktree);
    }

    if !fetch_all_ok {
        anyhow::bail!("git fetch origin failed; cannot create PR #{number} worktree");
    }

    let fetch = Command::new("git")
        .args([
            "-C",
            &bare_str,
            "fetch",
            "origin",
            &format!("pull/{number}/head:{branch}"),
        ])
        .output()
        .await
        .context("git fetch PR ref failed")?;
    if !fetch.status.success() {
        let stderr = String::from_utf8_lossy(&fetch.stderr);
        anyhow::bail!("git fetch PR ref failed for #{number}: {stderr}");
    }

    add_worktree_or_recover(&bare_str, &worktree, &branch)
        .await
        .with_context(|| format!("git worktree add failed for PR #{number}"))?;

    // Set upstream so `git push` targets the real PR branch, not origin/pr-<number>.
    let upstream = format!("origin/{head_branch}");
    let _ = Command::new("git")
        .args([
            "-C",
            &bare_str,
            "branch",
            "--set-upstream-to",
            &upstream,
            &branch,
        ])
        .output()
        .await;

    Ok(worktree)
}

/// Fetches the latest remote refs, then creates a fresh detached-HEAD investigation
/// worktree under `bare`. Returns the worktree path. The caller is responsible for
/// removing it via `git worktree remove --force` when done.
///
/// Use this at investigation launch time (CI, Issue) where the agent needs a
/// stable, isolated worktree starting from the latest trunk state. Prefer this over
/// opening the default-branch worktree directly — that worktree is reset
/// unconditionally every 30 minutes and at every investigation launch, so it is not
/// safe for any agent that runs longer than the refresh interval.
pub async fn fetch_and_create_investigation_worktree(bare: &Path) -> Result<PathBuf> {
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

    create_investigation_worktree(bare).await
}

/// Fetches the latest remote refs and syncs the default-branch worktree to them.
///
/// Unlike `ensure_default_branch_worktree` (which skips the fetch), this always
/// runs `git fetch origin` first so the subsequent reset lands on the actual
/// latest commit. Called only by the background fetch path (not investigations).
pub async fn sync_default_branch_worktree(bare: &Path) -> Result<()> {
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

    ensure_default_branch_worktree(bare).await
}

pub async fn ensure_default_branch_worktree(bare: &Path) -> Result<()> {
    let bare_str = bare.to_string_lossy().into_owned();
    let branch = read_default_branch(bare)
        .ok_or_else(|| anyhow::anyhow!("could not detect default branch in {bare_str}"))?;
    let worktree = bare.join(&branch);
    let worktree_str = worktree.to_string_lossy().into_owned();

    if !worktree.is_dir() {
        let add = Command::new("git")
            .args(["-C", &bare_str, "worktree", "add", &worktree_str, &branch])
            .output()
            .await
            .with_context(|| format!("git worktree add failed for {branch}"))?;
        if !add.status.success() {
            let stderr = String::from_utf8_lossy(&add.stderr);
            anyhow::bail!("git worktree add failed for {branch}: {stderr}");
        }
    }

    let reset = Command::new("git")
        .args([
            "-C",
            &worktree_str,
            "reset",
            "--hard",
            &format!("origin/{branch}"),
        ])
        .output()
        .await
        .with_context(|| format!("git reset failed for {branch} worktree"))?;
    if !reset.status.success() {
        let stderr = String::from_utf8_lossy(&reset.stderr);
        // Another git process is already updating this worktree; skip.
        if stderr.contains("index.lock") {
            return Ok(());
        }
        anyhow::bail!("git reset failed for {branch} worktree: {stderr}");
    }

    let clean = Command::new("git")
        .args(["-C", &worktree_str, "clean", "-fd"])
        .output()
        .await
        .with_context(|| format!("git clean failed for {branch} worktree"))?;
    if !clean.status.success() {
        let stderr = String::from_utf8_lossy(&clean.stderr);
        anyhow::bail!("git clean failed for {branch} worktree: {stderr}");
    }

    Ok(())
}

/// Creates a fresh detached-HEAD investigation worktree under `bare` at a
/// timestamped path. Returns the worktree path. The caller is responsible
/// for removing it via `git worktree remove --force` when done.
pub async fn create_investigation_worktree(bare: &Path) -> Result<PathBuf> {
    let bare_str = bare.to_string_lossy().into_owned();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let worktree = bare.join(format!("investigation-{ts}"));
    let worktree_str = worktree.to_string_lossy().into_owned();

    let add = Command::new("git")
        .args([
            "-C",
            &bare_str,
            "worktree",
            "add",
            "--detach",
            &worktree_str,
        ])
        .output()
        .await
        .context("git worktree add failed for investigation")?;
    if !add.status.success() {
        let stderr = String::from_utf8_lossy(&add.stderr);
        anyhow::bail!("git worktree add --detach failed: {stderr}");
    }

    Ok(worktree)
}

async fn clean_merged_worktrees(bare_dir: &str) -> Result<()> {
    let out = Command::new("git")
        .args(["-C", bare_dir, "worktree", "list", "--porcelain"])
        .output()
        .await
        .context("git worktree list failed")?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let worktrees = parse_worktree_list(&stdout);

    for (wt_path, branch) in worktrees.into_iter().skip(1) {
        let Some(branch) = branch else { continue };

        // Skip branches that were never pushed: no upstream means the missing
        // remote tracking ref is expected, not a signal that the branch was merged.
        let has_upstream = Command::new("git")
            .args(["-C", bare_dir, "config", &format!("branch.{branch}.remote")])
            .output()
            .await
            .context("git config branch.remote check failed")?
            .status
            .success();
        if !has_upstream {
            continue;
        }

        // PR worktrees use a local name like `pr-N` but track the real head
        // branch (e.g. `origin/feat/my-pr`). Read branch.<branch>.merge to find
        // the actual tracked branch rather than assuming it matches the local name.
        let merge_out = Command::new("git")
            .args(["-C", bare_dir, "config", &format!("branch.{branch}.merge")])
            .output()
            .await
            .context("git config branch.merge check failed")?;
        let tracking_branch = if merge_out.status.success() {
            let raw = String::from_utf8_lossy(&merge_out.stdout);
            raw.trim()
                .strip_prefix("refs/heads/")
                .map(|s| s.to_string())
                .unwrap_or_else(|| branch.clone())
        } else {
            branch.clone()
        };

        let remote_ref = format!("refs/remotes/origin/{tracking_branch}");
        let ref_gone = !Command::new("git")
            .args(["-C", bare_dir, "rev-parse", "--verify", &remote_ref])
            .output()
            .await
            .context("git rev-parse failed")?
            .status
            .success();

        if ref_gone {
            Command::new("git")
                .args(["-C", bare_dir, "worktree", "remove", "--force", &wt_path])
                .output()
                .await
                .context("git worktree remove failed")?;
        }
    }

    Ok(())
}

fn parse_worktree_list(output: &str) -> Vec<(String, Option<String>)> {
    let mut result = Vec::new();
    let mut cur_path: Option<String> = None;
    let mut cur_branch: Option<String> = None;

    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(p) = cur_path.take() {
                result.push((p, cur_branch.take()));
            }
            cur_path = Some(path.to_string());
            cur_branch = None;
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            cur_branch = Some(branch.to_string());
        }
    }
    if let Some(p) = cur_path {
        result.push((p, cur_branch));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_helpers::{git_rev_parse, push_commit, run_git, setup_origin_and_bare};

    #[test]
    fn bare_clone_path_uses_repo_name_not_project_nickname() {
        let repos = PathBuf::from("/tmp/repos");
        assert_eq!(
            bare_clone_path(&repos, "ooloth/hub").unwrap(),
            repos.join("hub")
        );
        assert_eq!(
            bare_clone_path(&repos, "org/legacy-name").unwrap(),
            repos.join("legacy-name")
        );
    }

    #[test]
    fn bare_clone_path_rejects_invalid_slug() {
        let repos = PathBuf::from("/tmp/repos");
        assert!(bare_clone_path(&repos, "not-a-slug").is_err());
    }

    #[tokio::test]
    async fn ensure_pr_worktree_reentry_fetches_remote_tracking_refs() {
        let tmp = tempfile::tempdir().unwrap();
        let (origin, bare) = setup_origin_and_bare(tmp.path()).await;

        // Simulate a previous investigation: pr-42/ dir already exists.
        std::fs::create_dir(bare.join("pr-42")).unwrap();

        // Push a new commit to origin — bare doesn't know about it yet.
        let new_sha = push_commit(&origin, "second").await;
        let before = git_rev_parse(&bare, "refs/remotes/origin/main").await;
        assert_ne!(
            before, new_sha,
            "setup: bare should be stale before the call"
        );

        ensure_pr_worktree(&bare, 42, "feat/test").await.unwrap();

        let after = git_rev_parse(&bare, "refs/remotes/origin/main").await;
        assert_eq!(
            after, new_sha,
            "ensure_pr_worktree must fetch origin on re-entry"
        );
    }

    #[tokio::test]
    async fn sync_default_branch_worktree_fetches_then_resets_to_latest_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let (origin, bare) = setup_origin_and_bare(tmp.path()).await;

        // Create the default-branch worktree at the initial commit.
        ensure_default_branch_worktree(&bare).await.unwrap();
        let worktree = bare.join("main");
        assert!(worktree.is_dir());

        // Push a new commit to origin.
        let new_sha = push_commit(&origin, "second").await;

        // Before sync: worktree HEAD and remote tracking ref are both stale.
        let wt_before = git_rev_parse(&worktree, "HEAD").await;
        assert_ne!(wt_before, new_sha, "setup: worktree should be stale");

        sync_default_branch_worktree(&bare).await.unwrap();

        // After sync: both the remote tracking ref and the worktree HEAD are current.
        let remote_after = git_rev_parse(&bare, "refs/remotes/origin/main").await;
        assert_eq!(remote_after, new_sha, "remote tracking ref must be updated");
        let wt_after = git_rev_parse(&worktree, "HEAD").await;
        assert_eq!(wt_after, new_sha, "worktree HEAD must be reset to latest");
    }

    #[test]
    fn default_branch_worktree_returns_none_when_worktree_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("HEAD"), "ref: refs/heads/main\n").unwrap();
        // No main/ subdir created
        assert!(default_branch_worktree(
            dir.path().parent().unwrap(),
            dir.path().file_name().unwrap().to_str().unwrap()
        )
        .is_none());
    }

    #[test]
    fn default_branch_worktree_returns_path_when_worktree_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let worktree = dir.path().join("main");
        std::fs::create_dir(&worktree).unwrap();
        let result = default_branch_worktree(
            dir.path().parent().unwrap(),
            dir.path().file_name().unwrap().to_str().unwrap(),
        );
        assert_eq!(result, Some(worktree));
    }

    #[test]
    fn parse_worktree_list_bare_only() {
        let input = "worktree /home/user/.hub/repos/hub\nHEAD abc123\nbare\n\n";
        let result = parse_worktree_list(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "/home/user/.hub/repos/hub");
        assert!(result[0].1.is_none());
    }

    #[test]
    fn parse_worktree_list_with_linked_worktree() {
        let input = concat!(
            "worktree /home/user/.hub/repos/hub\n",
            "HEAD abc123\n",
            "bare\n",
            "\n",
            "worktree /home/user/.hub/repos/hub/fix-ci-456\n",
            "HEAD def456\n",
            "branch refs/heads/fix-ci-456\n",
            "\n",
        );
        let result = parse_worktree_list(input);
        assert_eq!(result.len(), 2);
        assert_eq!(result[1].0, "/home/user/.hub/repos/hub/fix-ci-456");
        assert_eq!(result[1].1.as_deref(), Some("fix-ci-456"));
    }

    #[tokio::test]
    async fn clean_merged_worktrees_preserves_pr_worktree_while_head_branch_alive() {
        let tmp = tempfile::tempdir().unwrap();
        let (origin, bare) = setup_origin_and_bare(tmp.path()).await;
        let origin_str = origin.to_string_lossy().into_owned();
        let bare_str = bare.to_string_lossy().into_owned();

        // Create feat/my-pr on origin and fetch it into the bare repo.
        run_git(&["-C", &origin_str, "checkout", "-b", "feat/my-pr"]).await;
        run_git(&["-C", &origin_str, "commit", "--allow-empty", "-m", "pr"]).await;
        run_git(&["-C", &bare_str, "fetch", "origin"]).await;

        // Simulate the PR worktree setup: local branch pr-42 tracking origin/feat/my-pr.
        run_git(&[
            "-C",
            &bare_str,
            "branch",
            "pr-42",
            "refs/remotes/origin/feat/my-pr",
        ])
        .await;
        let wt = bare.join("pr-42");
        run_git(&[
            "-C",
            &bare_str,
            "worktree",
            "add",
            &wt.to_string_lossy(),
            "pr-42",
        ])
        .await;
        run_git(&[
            "-C",
            &bare_str,
            "branch",
            "--set-upstream-to",
            "origin/feat/my-pr",
            "pr-42",
        ])
        .await;

        clean_merged_worktrees(&bare_str).await.unwrap();

        assert!(
            wt.is_dir(),
            "pr worktree must survive while head branch exists on origin"
        );
    }

    #[tokio::test]
    async fn clean_merged_worktrees_removes_pr_worktree_after_head_branch_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let (origin, bare) = setup_origin_and_bare(tmp.path()).await;
        let origin_str = origin.to_string_lossy().into_owned();
        let bare_str = bare.to_string_lossy().into_owned();

        // Create feat/my-pr on origin, fetch, create PR worktree.
        run_git(&["-C", &origin_str, "checkout", "-b", "feat/my-pr"]).await;
        run_git(&["-C", &origin_str, "commit", "--allow-empty", "-m", "pr"]).await;
        run_git(&["-C", &bare_str, "fetch", "origin"]).await;
        run_git(&[
            "-C",
            &bare_str,
            "branch",
            "pr-42",
            "refs/remotes/origin/feat/my-pr",
        ])
        .await;
        let wt = bare.join("pr-42");
        run_git(&[
            "-C",
            &bare_str,
            "worktree",
            "add",
            &wt.to_string_lossy(),
            "pr-42",
        ])
        .await;
        run_git(&[
            "-C",
            &bare_str,
            "branch",
            "--set-upstream-to",
            "origin/feat/my-pr",
            "pr-42",
        ])
        .await;

        // Merge the PR: delete the head branch on origin and fetch --prune.
        run_git(&["-C", &origin_str, "checkout", "main"]).await;
        run_git(&["-C", &origin_str, "branch", "-D", "feat/my-pr"]).await;
        run_git(&["-C", &bare_str, "fetch", "--prune", "origin"]).await;

        clean_merged_worktrees(&bare_str).await.unwrap();

        assert!(
            !wt.is_dir(),
            "pr worktree must be removed after head branch is deleted"
        );
    }

    #[test]
    fn parse_worktree_list_detached_head_has_no_branch() {
        let input = concat!(
            "worktree /home/user/.hub/repos/hub\n",
            "HEAD abc123\n",
            "bare\n",
            "\n",
            "worktree /home/user/.hub/repos/hub/detached-wt\n",
            "HEAD def456\n",
            "detached\n",
            "\n",
        );
        let result = parse_worktree_list(input);
        assert_eq!(result.len(), 2);
        assert!(result[1].1.is_none());
    }
}
