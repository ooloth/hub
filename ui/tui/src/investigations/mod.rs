use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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
    /// Large supplemental data the agent should read. Written to a temp file
    /// before launch to avoid hitting OS argument-size limits that would occur
    /// if the data were inlined directly in `prompt`. The temp file path
    /// replaces `{SUPPORTING_DATA_PATH}` in `prompt`.
    pub(crate) supporting_data: Option<String>,
    pub(crate) model: String,
    pub(crate) allowed_tools: String,
    pub(crate) env: Vec<(String, String)>,
}

/// Describes the git worktree context an investigation agent runs in.
///
/// # Choosing the right variant
///
/// **Never route an investigation to the default-branch worktree directly.**
/// That worktree (`~/.hub/repos/<project>/<branch>/`) is reset unconditionally
/// to `origin/<branch>` every 30 minutes by the background fetch loop *and* again
/// at every investigation launch. Any agent working there longer than the refresh
/// interval — or running concurrently with another launch — will have its working
/// tree silently wiped mid-session.
///
/// | Scenario                                   | Variant to use   |
/// |--------------------------------------------|------------------|
/// | Read trunk state, may run >30 min (CI, Issue) | `EphemeralFresh` |
/// | PR-specific code (review, fix, ask)        | `PullRequest`    |
/// | Read-only log/infra investigation (GCP, Loki) | `Ephemeral`   |
/// | Non-git local process (media blocked)      | `CurrentDir`     |
pub(crate) enum WorktreeSpec {
    /// Fetch latest refs, then create a fresh detached-HEAD worktree from trunk.
    /// Cleaned up when the session exits. Use for investigations that need current
    /// trunk state but must not be disrupted by the background refresh (CI, Issue).
    EphemeralFresh { repo: String },
    /// Create or reuse a persistent PR worktree.
    PullRequest {
        repo: String,
        number: u64,
        head_branch: String,
    },
    /// Create a fresh detached-HEAD worktree from the most recently fetched trunk
    /// state, cleaned up when the session exits. Use for read-only log/infra
    /// investigations where a fresh fetch is not critical (GCP, Loki).
    /// `project` is the directory name under `~/.hub/repos/`.
    Ephemeral { project: String },
    /// Use the process's current directory (MediaBlocked, last-resort fallback).
    #[cfg(feature = "private")]
    CurrentDir,
}

pub(crate) async fn launch(
    config: LaunchConfig,
    spec: WorktreeSpec,
    hub_config: &config::Config,
) -> Result<()> {
    if std::env::var("TMUX").is_err() {
        bail!("not in tmux; investigation requires a tmux session");
    }

    let (cwd, cleanup) = resolve_worktree(spec, hub_config).await?;

    let prompt = match config.supporting_data {
        Some(ref data) => {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = format!("/tmp/hub-supporting-data-{nanos}.json");
            std::fs::write(&path, data)
                .with_context(|| format!("failed to write supporting data to {path}"))?;
            config.prompt.replace("{SUPPORTING_DATA_PATH}", &path)
        }
        None => config.prompt,
    };

    let task_arg = if prompt.is_empty() {
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
    cmd.arg("-e").arg(format!("HUB_TASK_PROMPT={prompt}"));
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

pub(crate) async fn open_in_lazygit(
    repo: &str,
    number: u64,
    head_branch: &str,
    hub_config: &config::Config,
) -> Result<()> {
    if std::env::var("TMUX").is_err() {
        bail!("not in tmux; opening lazygit requires a tmux session");
    }

    let name = project_name(hub_config, repo)?;
    let bare = workflows::fetch::repos_dir().join(name);
    if !bare.exists() {
        bail!("repo not synced; open the TUI to fetch it");
    }
    let cwd = workflows::fetch::ensure_pr_worktree(&bare, number, head_branch)
        .await
        .context("Failed to create PR worktree")?;

    let repo_name = repo.split_once('/').map(|(_, name)| name).unwrap_or(repo);
    let window_name = format!("{repo_name}#{number}-git");

    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["new-window", "-n", &window_name, "-c"])
        .arg(&cwd)
        .arg("lazygit");

    let status = cmd.status().context("failed to start tmux new-window")?;
    if !status.success() {
        bail!("tmux new-window failed with {status}");
    }

    Ok(())
}

pub(crate) async fn open_in_octo(
    repo: &str,
    number: u64,
    head_branch: &str,
    hub_config: &config::Config,
) -> Result<()> {
    if std::env::var("TMUX").is_err() {
        bail!("not in tmux; opening in neovim requires a tmux session");
    }

    let name = project_name(hub_config, repo)?;
    let bare = workflows::fetch::repos_dir().join(name);
    if !bare.exists() {
        bail!("repo not synced; open the TUI to fetch it");
    }
    let cwd = workflows::fetch::ensure_pr_worktree(&bare, number, head_branch)
        .await
        .context("Failed to create PR worktree")?;

    let repo_name = repo.split_once('/').map(|(_, name)| name).unwrap_or(repo);
    let window_name = format!("{repo_name}#{number}");

    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["new-window", "-n", &window_name, "-c"])
        .arg(&cwd)
        .arg(format!(
            "NVIM_APPNAME=nvim-ide nvim +'Octo pr edit {number}'"
        ));

    let status = cmd.status().context("failed to start tmux new-window")?;
    if !status.success() {
        bail!("tmux new-window failed with {status}");
    }

    Ok(())
}

async fn resolve_worktree(
    spec: WorktreeSpec,
    hub_config: &config::Config,
) -> Result<(PathBuf, Option<String>)> {
    match spec {
        WorktreeSpec::EphemeralFresh { repo } => {
            let name = project_name(hub_config, &repo)?;
            let bare = workflows::fetch::repos_dir().join(name);
            if !bare.exists() {
                bail!("repo not synced; open the TUI to fetch it");
            }
            let worktree = workflows::fetch::fetch_and_create_investigation_worktree(&bare)
                .await
                .context("Failed to create investigation worktree")?;
            let cleanup = format!(
                "cd ~ && git -C '{}' worktree remove --force '{}' 2>/dev/null || true",
                bare.display(),
                worktree.display()
            );
            Ok((worktree, Some(cleanup)))
        }
        WorktreeSpec::PullRequest {
            repo,
            number,
            head_branch,
        } => {
            let name = project_name(hub_config, &repo)?;
            let bare = workflows::fetch::repos_dir().join(name);
            if !bare.exists() {
                bail!("repo not synced; open the TUI to fetch it");
            }
            let cwd = workflows::fetch::ensure_pr_worktree(&bare, number, &head_branch)
                .await
                .context("Failed to create PR worktree")?;
            Ok((cwd, None))
        }
        WorktreeSpec::Ephemeral { project } => {
            let bare = workflows::fetch::repos_dir().join(&project);
            if !bare.exists() {
                bail!("no repo at {}; open the TUI to fetch it", bare.display());
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
        #[cfg(feature = "private")]
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
