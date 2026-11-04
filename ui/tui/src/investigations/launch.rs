use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use domain::{InvestigationPrompt, UntrustedText};

use crate::terminal::{claim, WindowClaim, WindowPlace};

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
/// `WorktreeSpec` is consumed by `launch()`. `open_in_lazygit` and `open_in_octo`
/// open the PR worktree themselves. See `docs/architecture/worktrees.md` for how
/// these worktrees are laid out and cleaned up.
///
/// # Choosing the right variant
///
/// **Never route an investigation to the default-branch worktree directly.**
/// That worktree (`~/.hub/repos/<repo name>/<branch>/`) is reset unconditionally
/// to `origin/<branch>` by every refresh. Any agent working there longer than the refresh
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
    EphemeralFresh { project: config::Project },
    /// Create or reuse a persistent PR worktree.
    PullRequest {
        project: config::Project,
        number: u64,
        head_branch: String,
    },
    /// Create a fresh detached-HEAD worktree from the most recently fetched trunk
    /// state, cleaned up when the session exits. Use for read-only log/infra
    /// investigations where a fresh fetch is not critical (GCP, Loki).
    Ephemeral { project: config::Project },
    /// Use the process's current directory (`MediaBlocked`, last-resort fallback).
    #[cfg(feature = "private")]
    CurrentDir,
}

/// Opens an investigation in its own tmux window or Rex tab, or finds the window
/// already investigating this signal, and returns where it is.
///
/// `window` names the window, and naming it is what makes it addressable
/// afterwards: several investigations run side by side, each in its own window
/// or tab, without subdividing the TUI's own pane.
///
/// The claim comes before `resolve_worktree`, and that ordering is load bearing.
/// `WorktreeSpec::EphemeralFresh` and `Ephemeral` create a worktree whose
/// cleanup is appended to the command the window runs, so one resolved for a launch
/// that then finds an open window instead would have nothing to remove it. A window
/// that fails to open never runs that command, so its worktree is removed here, along
/// with its supporting-data file.
pub(crate) async fn launch(
    config: LaunchConfig,
    window: &domain::InvestigationWindow,
    spec: WorktreeSpec,
    hub_config: &config::Config,
) -> Result<WindowPlace> {
    let vacant = match claim(window, "investigation")? {
        WindowClaim::Reused(place) => return Ok(place),
        WindowClaim::Vacant(vacant) => vacant,
    };

    let (cwd, ephemeral) = resolve_worktree(spec).await?;

    let supporting_data_path = match config.supporting_data {
        Some(ref data) => match write_supporting_data(data) {
            Ok(path) => Some(path),
            Err(err) => return Err(discard(err, ephemeral.as_ref(), None).await),
        },
        None => None,
    };

    let cleanup = ephemeral.as_ref().map(EphemeralWorktree::removal_command);
    let command = compose(
        config,
        &cwd,
        &hub_config.investigation_mcp_servers,
        cleanup.as_deref(),
        supporting_data_path.as_deref(),
    );

    match vacant.open(Some(&command.cwd), &command.env, &command.shell) {
        Ok(place) => Ok(place),
        Err(err) => Err(discard(err, ephemeral.as_ref(), supporting_data_path.as_deref()).await),
    }
}

/// Writes `data` where the investigation's agent will read it, returning the path.
fn write_supporting_data(data: &SupportingData) -> Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = PathBuf::from(format!("/tmp/hub-supporting-data-{nanos}.json"));
    std::fs::write(&path, data.file_contents())
        .with_context(|| format!("failed to write supporting data to {}", path.display()))?;
    Ok(path)
}

/// Removes what a launch made for a window that never opened, and returns the launch's error.
///
/// The worktree's only other cleanup is the command its window runs once the agent exits, and
/// a window that failed to open never runs it. A removal that fails too is added to the
/// error, so the status bar names what was left behind.
async fn discard(
    err: anyhow::Error,
    ephemeral: Option<&EphemeralWorktree>,
    supporting_data: Option<&Path>,
) -> anyhow::Error {
    let mut left = Vec::new();
    if let Some(worktree) = ephemeral {
        if let Err(removal) =
            workflows::fetch::remove_investigation_worktree(&worktree.bare, &worktree.path).await
        {
            left.push(format!("{removal:#}"));
        }
    }
    if let Some(path) = supporting_data {
        if let Err(removal) = std::fs::remove_file(path) {
            left.push(format!("couldn't remove {}: {removal}", path.display()));
        }
    }
    match left.as_slice() {
        [] => err,
        _ => err.context(format!(
            "left behind after the failed launch: {}",
            left.join("; ")
        )),
    }
}

/// A worktree made for one investigation, removed when its window's command ends.
struct EphemeralWorktree {
    bare: PathBuf,
    path: PathBuf,
}

impl EphemeralWorktree {
    /// The shell command the window runs after the agent exits. A launch whose window never
    /// opened removes the worktree with `remove_investigation_worktree` instead.
    fn removal_command(&self) -> String {
        format!(
            "cd ~ && git -C '{}' worktree remove --force '{}' 2>/dev/null || true",
            self.bare.display(),
            self.path.display()
        )
    }
}

pub(crate) async fn open_in_lazygit(
    project: &config::Project,
    number: u64,
    head_branch: &str,
) -> Result<WindowPlace> {
    let vacant = match claim(
        &domain::InvestigationWindow::lazygit(&project.short_name, &project.repo, number),
        "opening lazygit",
    )? {
        WindowClaim::Reused(place) => return Ok(place),
        WindowClaim::Vacant(vacant) => vacant,
    };

    let bare = synced_clone(project)?;
    let cwd = workflows::fetch::ensure_pr_worktree(&bare, number, head_branch)
        .await
        .context("Failed to create PR worktree")?;

    vacant.open(Some(&cwd), &[], "lazygit")
}

pub(crate) async fn open_in_octo(
    project: &config::Project,
    number: u64,
    head_branch: &str,
) -> Result<WindowPlace> {
    let vacant = match claim(
        &domain::InvestigationWindow::octo(&project.short_name, &project.repo, number),
        "opening in neovim",
    )? {
        WindowClaim::Reused(place) => return Ok(place),
        WindowClaim::Vacant(vacant) => vacant,
    };

    let bare = synced_clone(project)?;
    let cwd = workflows::fetch::ensure_pr_worktree(&bare, number, head_branch)
        .await
        .context("Failed to create PR worktree")?;

    vacant.open(
        Some(&cwd),
        &[],
        &format!("NVIM_APPNAME=nvim-ide nvim +'Octo pr edit {number}'"),
    )
}

async fn resolve_worktree(spec: WorktreeSpec) -> Result<(PathBuf, Option<EphemeralWorktree>)> {
    match spec {
        WorktreeSpec::EphemeralFresh { project } => {
            let bare = synced_clone(&project)?;
            let path = workflows::fetch::fetch_and_create_investigation_worktree(&bare)
                .await
                .context("Failed to create investigation worktree")?;
            Ok((path.clone(), Some(EphemeralWorktree { bare, path })))
        }
        WorktreeSpec::PullRequest {
            project,
            number,
            head_branch,
        } => {
            let bare = synced_clone(&project)?;
            let cwd = workflows::fetch::ensure_pr_worktree(&bare, number, &head_branch)
                .await
                .context("Failed to create PR worktree")?;
            Ok((cwd, None))
        }
        WorktreeSpec::Ephemeral { project } => {
            let bare = synced_clone(&project)?;
            let path = workflows::fetch::create_investigation_worktree(&bare)
                .await
                .context("Failed to create investigation worktree")?;
            Ok((path.clone(), Some(EphemeralWorktree { bare, path })))
        }
        #[cfg(feature = "private")]
        WorktreeSpec::CurrentDir => {
            let cwd = std::env::current_dir().context("Cannot determine working directory")?;
            Ok((cwd, None))
        }
    }
}

/// The bare clone of a tracked project, which must already have been fetched.
///
/// Taking the project rather than a repo is what keeps a launch for a
/// repository hub does not track out of a tracked one's clone of the same name.
fn synced_clone(project: &config::Project) -> Result<PathBuf> {
    let repo = &project.repo;
    let bare = workflows::fetch::bare_clone(repo);
    if !bare.exists() {
        bail!(
            "no clone of {repo} at {}: hub clones each [[project]] in hub.toml when it refreshes",
            bare.display()
        );
    }
    Ok(bare)
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::RepoSlug;

    /// A launch whose window never opened removes the supporting-data file it wrote, and the
    /// status bar still says why the window didn't open.
    #[tokio::test]
    async fn a_launch_that_could_not_open_removes_its_supporting_data() {
        let dir = std::env::temp_dir().join(format!("hub-discard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("supporting-data.json");
        std::fs::write(&path, "{}").unwrap();

        let err = discard(anyhow::anyhow!("rex session new failed"), None, Some(&path)).await;

        assert!(!path.exists(), "{} is still there", path.display());
        assert_eq!(format!("{err:#}"), "rex session new failed");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// When the removal fails too, the error names what was left behind.
    #[tokio::test]
    async fn what_a_failed_launch_could_not_remove_is_named() {
        let path = std::env::temp_dir().join(format!(
            "hub-discard-never-written-{}.json",
            std::process::id()
        ));

        let err = discard(anyhow::anyhow!("rex session new failed"), None, Some(&path)).await;

        let shown = format!("{err:#}");
        assert!(
            shown.starts_with("left behind after the failed launch: couldn't remove ")
                && shown.contains("hub-discard-never-written-")
                && shown.ends_with(": rex session new failed"),
            "{shown}"
        );
    }

    /// A tracked repo that has not been fetched yet has no clone, so the error
    /// has to say which repo and where it looked rather than only that it failed.
    #[test]
    fn a_tracked_repo_with_no_clone_is_an_error_naming_the_repo_and_the_path() {
        let repo: RepoSlug = "hub-tests/no-such-clone".parse().unwrap();
        let project = config::Project {
            name: "no-such-clone".to_string(),
            short_name: domain::ProjectShortName::from_repo(&repo),
            repo: repo.clone(),
            workflow: vec![],
            environment: vec![],
        };

        let error = synced_clone(&project).unwrap_err().to_string();

        assert!(error.contains("hub-tests/no-such-clone"), "{error}");
        assert!(
            error.contains(&workflows::fetch::bare_clone(&repo).display().to_string()),
            "{error}"
        );
    }
}
