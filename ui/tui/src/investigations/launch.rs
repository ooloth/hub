use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use domain::{InvestigationPrompt, UntrustedText};

use crate::tmux::{claim, WindowClaim};

use super::command::compose;

/// Foreign text too large for a prompt, handed to the agent as a file.
///
/// The file is the second way hub puts text it did not write in front of an
/// agent, so it is fenced exactly as a prompt segment is. `source` travels with
/// the body because the file is written by `launch`, which has no idea what
/// kind of signal produced it.
pub(crate) struct SupportingData {
    /// Human-readable provenance, e.g. "loki log lines".
    pub(crate) source: String,
    /// The text itself.
    pub(crate) body: UntrustedText,
}

impl SupportingData {
    /// What gets written to the file.
    pub(crate) fn file_contents(&self) -> String {
        domain::fenced(&self.source, &self.body)
    }
}

pub(crate) struct LaunchConfig {
    pub(crate) system_prompt: String,
    pub(crate) prompt: InvestigationPrompt,
    /// Large supplemental data the agent should read. Written to a temp file
    /// before launch to avoid hitting OS argument-size limits that would occur
    /// if the data were inlined directly in `prompt`. The prompt refers to the
    /// file through a `SupportingDataPath` segment.
    pub(crate) supporting_data: Option<SupportingData>,
    pub(crate) model: String,
    pub(crate) allowed_tools: String,
    pub(crate) env: Vec<(String, String)>,
}

/// Describes the git worktree context an **investigation** agent runs in.
///
/// # This enum is for human-interactive investigation sessions only
///
/// `WorktreeSpec` is consumed by `launch()` → `open_in_lazygit` / `open_in_octo`.
/// See `docs/architecture/worktrees.md` for how these worktrees are laid out
/// and cleaned up.
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
    /// Use the process's current directory (`MediaBlocked`, last-resort fallback).
    #[cfg(feature = "private")]
    CurrentDir,
}

/// Opens an investigation in its own tmux window, or switches to the window
/// already investigating this signal.
///
/// `window` names the window, and naming it is what makes it addressable
/// afterwards: several investigations run side by side, each reachable from
/// tmux's window list, without subdividing the TUI's own pane.
///
/// The claim comes before `resolve_worktree`, and that ordering is load bearing.
/// `WorktreeSpec::EphemeralFresh` and `Ephemeral` create a worktree whose only
/// cleanup is appended to the command tmux runs, so one resolved for a launch
/// that then switches instead would have nothing to remove it.
pub(crate) async fn launch(
    config: LaunchConfig,
    window: &domain::InvestigationWindow,
    spec: WorktreeSpec,
    hub_config: &config::Config,
) -> Result<()> {
    let vacant = match claim(window, "investigation")? {
        WindowClaim::Reused => return Ok(()),
        WindowClaim::Vacant(vacant) => vacant,
    };

    let (cwd, cleanup) = resolve_worktree(spec, hub_config).await?;

    let supporting_data_path = match config.supporting_data {
        Some(ref data) => {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = PathBuf::from(format!("/tmp/hub-supporting-data-{nanos}.json"));
            std::fs::write(&path, data.file_contents()).with_context(|| {
                format!("failed to write supporting data to {}", path.display())
            })?;
            Some(path)
        }
        None => None,
    };

    let command = compose(
        config,
        &cwd,
        cleanup.as_deref(),
        supporting_data_path.as_deref(),
    );

    vacant.open(Some(&command.cwd), &command.env, &command.shell)
}

pub(crate) async fn open_in_lazygit(
    repo: &str,
    number: u64,
    head_branch: &str,
    hub_config: &config::Config,
) -> Result<()> {
    let vacant = match claim(
        &domain::InvestigationWindow::lazygit(repo, number),
        "opening lazygit",
    )? {
        WindowClaim::Reused => return Ok(()),
        WindowClaim::Vacant(vacant) => vacant,
    };

    let name = project_name(hub_config, repo)?;
    let bare = workflows::fetch::repos_dir().join(name);
    if !bare.exists() {
        bail!("repo not synced; open the TUI to fetch it");
    }
    let cwd = workflows::fetch::ensure_pr_worktree(&bare, number, head_branch)
        .await
        .context("Failed to create PR worktree")?;

    vacant.open(Some(&cwd), &[], "lazygit")
}

pub(crate) async fn open_in_octo(
    repo: &str,
    number: u64,
    head_branch: &str,
    hub_config: &config::Config,
) -> Result<()> {
    let vacant = match claim(
        &domain::InvestigationWindow::octo(repo, number),
        "opening in neovim",
    )? {
        WindowClaim::Reused => return Ok(()),
        WindowClaim::Vacant(vacant) => vacant,
    };

    let name = project_name(hub_config, repo)?;
    let bare = workflows::fetch::repos_dir().join(name);
    if !bare.exists() {
        bail!("repo not synced; open the TUI to fetch it");
    }
    let cwd = workflows::fetch::ensure_pr_worktree(&bare, number, head_branch)
        .await
        .context("Failed to create PR worktree")?;

    vacant.open(
        Some(&cwd),
        &[],
        &format!("NVIM_APPNAME=nvim-ide nvim +'Octo pr edit {number}'"),
    )
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
