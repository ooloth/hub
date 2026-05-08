use anyhow::{Context, Result};
use std::{collections::HashSet, path::Path, path::PathBuf};
use tokio::process::Command;

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

fn read_default_branch(bare: &Path) -> Option<String> {
    let head = std::fs::read_to_string(bare.join("HEAD")).ok()?;
    let branch = head.trim().strip_prefix("ref: refs/heads/")?;
    Some(branch.to_owned())
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
pub async fn run(projects: &[(String, String)], github_token: &str) -> Result<()> {
    let dir = repos_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .context("failed to create ~/.hub/repos")?;

    let names: HashSet<&str> = projects.iter().map(|(n, _)| n.as_str()).collect();
    warn_orphans(&dir, &names);

    for (name, repo) in projects {
        fetch_project(name, repo, github_token, &dir).await?;
    }

    Ok(())
}

async fn fetch_project(name: &str, repo: &str, github_token: &str, repos_dir: &Path) -> Result<()> {
    let dir = repos_dir.join(name);
    let dir_str = dir.to_string_lossy().into_owned();
    let clean_url = format!("https://github.com/{repo}.git");
    // url.insteadOf rewrites the clean URL to an authenticated one at call time
    // without persisting the token in the stored remote config.
    let rewrite = format!(
        "url.https://x-access-token:{github_token}@github.com/.insteadOf=https://github.com/"
    );

    if dir.exists() {
        let out = Command::new("git")
            .args(["-C", &dir_str, "-c", &rewrite, "fetch", "--prune", "origin"])
            .env("GIT_TERMINAL_PROMPT", "0")
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
        let authed_url = format!("https://x-access-token:{github_token}@github.com/{repo}.git");
        let out = Command::new("git")
            .args(["clone", "--bare", &authed_url, &dir_str])
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .await
            .with_context(|| format!("git clone failed for {name}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("git clone failed for {name}: {stderr}");
        }
        // Reset the stored remote URL to the clean (token-free) form.
        let reset = Command::new("git")
            .args(["-C", &dir_str, "remote", "set-url", "origin", &clean_url])
            .output()
            .await
            .with_context(|| format!("git remote set-url failed for {name}"))?;
        if !reset.status.success() {
            let stderr = String::from_utf8_lossy(&reset.stderr);
            anyhow::bail!("git remote set-url failed for {name}: {stderr}");
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
            .args(["-C", &dir_str, "-c", &rewrite, "fetch", "origin"])
            .env("GIT_TERMINAL_PROMPT", "0")
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
        let remote_ref = format!("refs/remotes/origin/{branch}");

        let check = Command::new("git")
            .args(["-C", bare_dir, "rev-parse", "--verify", &remote_ref])
            .output()
            .await
            .context("git rev-parse failed")?;

        if !check.status.success() {
            let rm = Command::new("git")
                .args(["-C", bare_dir, "worktree", "remove", "--force", &wt_path])
                .output()
                .await
                .context("git worktree remove failed")?;
            if rm.status.success() {
                eprintln!("hub fetch: removed merged worktree {branch}");
            }
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

fn warn_orphans(repos_dir: &Path, project_names: &HashSet<&str>) {
    let Ok(entries) = std::fs::read_dir(repos_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            if let Some(dir_name) = entry.file_name().to_str() {
                if !project_names.contains(dir_name) {
                    eprintln!(
                        "warning: ~/.hub/repos/{dir_name} is not in hub.toml (orphaned clone)"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
