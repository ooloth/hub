use anyhow::{bail, Context, Result};
use std::path::PathBuf;

pub(crate) mod ci;
pub(crate) mod gcp;
pub(crate) mod issue;
pub(crate) mod loki;
pub(crate) mod pr;
// Device-specific: home-laptop only via hub-private symlink; setup-private creates
// a stub on other devices so the file always exists when private is enabled.
#[cfg(feature = "private")]
pub(crate) mod media;

pub(crate) struct LaunchConfig {
    pub(crate) system_prompt: String,
    pub(crate) prompt: String,
    pub(crate) model: String,
    pub(crate) allowed_tools: String,
    pub(crate) env: Vec<(String, String)>,
}

/// Describes the git worktree context an investigation agent runs in.
pub(crate) enum WorktreeSpec {
    /// Sync and use the default-branch worktree for a GitHub repo (CI, Issue).
    DefaultBranch { repo: String },
    /// Create or reuse a persistent PR worktree.
    PullRequest {
        repo: String,
        number: u64,
        head_branch: String,
    },
    /// Create a fresh detached-HEAD worktree, cleaned up when the session exits (GCP, Loki).
    /// `project` is the directory name under `~/.hub/repos/`.
    Ephemeral { project: String },
    /// Use the process's current directory (MediaBlocked, last-resort fallback).
    CurrentDir,
}

pub(crate) async fn launch(
    config: LaunchConfig,
    spec: WorktreeSpec,
    hub_config: &config::Config,
    github_token: &str,
) -> Result<()> {
    if std::env::var("TMUX").is_err() {
        bail!("not in tmux; investigation requires a tmux session");
    }

    let (cwd, cleanup) = resolve_worktree(spec, hub_config, github_token).await?;

    let task_arg = if config.prompt.is_empty() {
        String::new()
    } else {
        " \"$HUB_TASK_PROMPT\"".to_string()
    };

    let cleanup_suffix = cleanup
        .as_deref()
        .map(|c| format!("; {c}"))
        .unwrap_or_default();
    let command = format!(
        "claude --dangerously-skip-permissions --model {} --allowedTools '{}' --append-system-prompt \"$HUB_SYSTEM_PROMPT\"{}{cleanup_suffix}",
        config.model,
        config.allowed_tools,
        task_arg,
    );

    let pane = std::env::var("TMUX_PANE").unwrap_or_default();

    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["split-window", "-h", "-t", &pane, "-c"])
        .arg(&cwd);
    cmd.arg("-e")
        .arg(format!("HUB_SYSTEM_PROMPT={}", config.system_prompt));
    cmd.arg("-e")
        .arg(format!("HUB_TASK_PROMPT={}", config.prompt));
    cmd.arg("-e").arg("GIT_TERMINAL_PROMPT=0");
    cmd.arg("-e").arg(format!(
        "GIT_CONFIG_PARAMETERS='url.https://x-access-token:{github_token}@github.com/.insteadOf=https://github.com/'"
    ));
    for (k, v) in &config.env {
        cmd.arg("-e").arg(format!("{k}={v}"));
    }
    cmd.arg(&command);

    let status = cmd.status().context("failed to start tmux split-window")?;

    if !status.success() {
        bail!("tmux split-window failed with {status}");
    }

    Ok(())
}

async fn resolve_worktree(
    spec: WorktreeSpec,
    hub_config: &config::Config,
    github_token: &str,
) -> Result<(PathBuf, Option<String>)> {
    match spec {
        WorktreeSpec::DefaultBranch { repo } => {
            let name = project_name(hub_config, &repo)?;
            let repos = workflows::fetch::repos_dir();
            let bare = repos.join(name);
            if !bare.exists() {
                bail!("Not fetched yet; run hub fetch");
            }
            workflows::fetch::sync_default_branch_worktree(&bare, github_token)
                .await
                .context("Failed to sync worktree")?;
            let cwd = workflows::fetch::default_branch_worktree(&repos, name)
                .ok_or_else(|| anyhow::anyhow!("Worktree sync succeeded but path not found"))?;
            Ok((cwd, None))
        }
        WorktreeSpec::PullRequest {
            repo,
            number,
            head_branch,
        } => {
            let name = project_name(hub_config, &repo)?;
            let bare = workflows::fetch::repos_dir().join(name);
            if !bare.exists() {
                bail!("Not fetched yet; run hub fetch");
            }
            let cwd =
                workflows::fetch::ensure_pr_worktree(&bare, number, &head_branch, github_token)
                    .await
                    .context("Failed to create PR worktree")?;
            Ok((cwd, None))
        }
        WorktreeSpec::Ephemeral { project } => {
            let bare = workflows::fetch::repos_dir().join(&project);
            if !bare.exists() {
                bail!("No repo at {}; run hub fetch", bare.display());
            }
            let worktree = workflows::fetch::create_investigation_worktree(&bare)
                .await
                .context("Failed to create investigation worktree")?;
            let cleanup = format!(
                "cd ~ && git -C '{}' worktree remove --force '{}' 2>/dev/null || true",
                bare.display(),
                worktree.display()
            );
            Ok((worktree, Some(cleanup)))
        }
        WorktreeSpec::CurrentDir => {
            let cwd = std::env::current_dir().context("Cannot determine working directory")?;
            Ok((cwd, None))
        }
    }
}

fn project_name<'a>(hub_config: &'a config::Config, repo: &str) -> Result<&'a str> {
    hub_config
        .projects
        .iter()
        .find(|p| p.repo == repo)
        .map(|p| p.name.as_str())
        .ok_or_else(|| anyhow::anyhow!("No project found for {repo}"))
}
