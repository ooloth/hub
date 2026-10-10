//! The `daemon` subcommands: each reads what launchd and the disk hold, plans, then acts.
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Subcommand;

use super::definition::{definition_path, AgentDefinition, DefinitionOnDisk, LABEL};
use super::launch_agent::{
    AbsolutePath, CallerPath, InstalledDaemon, LaunchAgent, LogFile, MainCheckout, SearchPath,
};
use super::plan::{plan, AgentOutcome, AgentStep, JobState, Situation};
use crate::repo_root::repo_root;

/// Running `hub-daemon` under launchd for the `default` profile.
#[derive(Subcommand)]
pub(crate) enum DaemonCommand {
    /// Install the `LaunchAgent` and start the daemon. It then starts at every login.
    ///
    /// Rewrites the agent definition only when it changed, and reloads a running daemon only
    /// then. Needs `hub-daemon` on PATH (`just install`) and `hub.toml` in the main checkout.
    Start,
    /// Stop the daemon until the next login. The agent definition stays installed.
    Stop,
    /// Follow the daemon's log: startup lines, startup errors and one line per pass.
    Logs,
}

/// `launchctl print` exits with this when the domain has no job by that label.
const LAUNCHCTL_NO_SUCH_JOB: i32 = 113;

/// How long `bootout` may take to finish unloading the job. It returns before launchd has
/// removed it, and a `bootstrap` sent in that window fails with "Input/output error".
const UNLOAD_TIMEOUT: Duration = Duration::from_secs(10);
const UNLOAD_POLL: Duration = Duration::from_millis(100);

pub(crate) fn run(command: &DaemonCommand) -> ExitCode {
    let outcome = match command {
        DaemonCommand::Start => start(),
        DaemonCommand::Stop => stop(),
        DaemonCommand::Logs => follow_logs(),
    };
    match outcome {
        Ok(outcome) => {
            println!("{outcome}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn start() -> Result<AgentOutcome> {
    let home = AbsolutePath::new(Path::new(
        &std::env::var_os("HOME").context("HOME is unset")?,
    ))
    .context("HOME cannot hold the daemon's files")?;
    let caller = CallerPath::new(std::env::var("PATH").ok())?;
    let agent = LaunchAgent {
        program: InstalledDaemon::find(&caller)?,
        checkout: MainCheckout::of(&git_common_dir(&repo_root())?)?,
        search_path: SearchPath::for_daemon(&caller),
        log: LogFile::of_default_profile(&home)?,
    };
    let rendered = AgentDefinition::render(&agent)?;
    let installed_at = definition_path(&home);
    let installed = match std::fs::read(&installed_at) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", installed_at.display()))
        }
    };
    let domain = gui_domain()?;
    let situation = Situation::Starting {
        job: job_state(&domain)?,
        on_disk: DefinitionOnDisk::compare(installed.as_deref(), &rendered),
    };
    let steps = plan(situation);

    // launchd creates the log file but not its directory, and the daemon creates the profile
    // directory only after launchd has already opened the log.
    if let Some(dir) = agent.log.path().as_path().parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
    }
    for step in &steps {
        match step {
            AgentStep::WriteDefinition => {
                if let Some(dir) = installed_at.parent() {
                    std::fs::create_dir_all(dir)
                        .with_context(|| format!("failed to create {}", dir.display()))?;
                }
                std::fs::write(&installed_at, rendered.bytes())
                    .with_context(|| format!("failed to write {}", installed_at.display()))?;
            }
            AgentStep::Bootout => bootout(&domain)?,
            AgentStep::Bootstrap => launchctl(&[
                "bootstrap",
                &domain,
                installed_at
                    .to_str()
                    .context("the agent definition's path is not UTF-8")?,
            ])?,
        }
    }
    if steps.contains(&AgentStep::Bootstrap) && job_state(&domain)? != JobState::Loaded {
        bail!(
            "launchd accepted {} but did not load it; check that hub-daemon is allowed in \
             System Settings > General > Login Items & Extensions",
            installed_at.display()
        );
    }
    Ok(AgentOutcome::of(situation, &steps))
}

fn stop() -> Result<AgentOutcome> {
    let domain = gui_domain()?;
    let situation = Situation::Stopping {
        job: job_state(&domain)?,
    };
    let steps = plan(situation);
    for step in &steps {
        match step {
            AgentStep::Bootout => bootout(&domain)?,
            AgentStep::WriteDefinition | AgentStep::Bootstrap => {
                unreachable!("stopping planned {step:?}")
            }
        }
    }
    Ok(AgentOutcome::of(situation, &steps))
}

/// Replaces this process with `tail`, so the log is followed until interrupted.
fn follow_logs() -> Result<AgentOutcome> {
    let home = AbsolutePath::new(Path::new(
        &std::env::var_os("HOME").context("HOME is unset")?,
    ))?;
    let log = LogFile::of_default_profile(&home)?;
    let error = Command::new("tail")
        .args(["-n", "200", "-F", log.path().as_str()])
        .exec();
    Err(error).context("failed to run tail")
}

/// The launchd domain of the logged-in user's GUI session, where `LaunchAgents` run.
fn gui_domain() -> Result<String> {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .context("failed to run id -u")?;
    if !output.status.success() {
        bail!("id -u failed ({})", output.status);
    }
    let uid = String::from_utf8(output.stdout).context("id -u printed non-UTF-8")?;
    Ok(format!("gui/{}", uid.trim()))
}

/// Whether launchd has the job loaded in `domain`.
fn job_state(domain: &str) -> Result<JobState> {
    let status = Command::new("launchctl")
        .args(["print", &format!("{domain}/{LABEL}")])
        .output()
        .context("failed to run launchctl print")?
        .status;
    match status.code() {
        Some(0) => Ok(JobState::Loaded),
        Some(LAUNCHCTL_NO_SUCH_JOB) => Ok(JobState::NotLoaded),
        _ => bail!("launchctl print {domain}/{LABEL} failed ({status})"),
    }
}

/// Unloads the job and returns once launchd no longer has it.
fn bootout(domain: &str) -> Result<()> {
    launchctl(&["bootout", &format!("{domain}/{LABEL}")])?;
    let started = Instant::now();
    while job_state(domain)? == JobState::Loaded {
        if started.elapsed() > UNLOAD_TIMEOUT {
            bail!(
                "launchd still has {LABEL} loaded {} s after bootout",
                UNLOAD_TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(UNLOAD_POLL);
    }
    Ok(())
}

fn launchctl(args: &[&str]) -> Result<()> {
    let output = Command::new("launchctl")
        .args(args)
        .output()
        .with_context(|| format!("failed to run launchctl {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "launchctl {} failed ({}): {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Git's shared directory for the repository at `repo`, the same from the main checkout and
/// from any of its worktrees.
///
/// # Errors
/// Returns an error when git cannot say, or prints a path launchd could not use.
fn git_common_dir(repo: &Path) -> Result<AbsolutePath> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo)
        .output()
        .context("failed to run git rev-parse")?;
    if !output.status.success() {
        bail!(
            "{} is not a git checkout: {}",
            repo.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let printed = String::from_utf8(output.stdout).context("git printed non-UTF-8")?;
    AbsolutePath::new(Path::new(printed.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn a_worktree_shares_the_main_checkouts_git_directory() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("hub");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(
            &main,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        let worktree = dir.path().join("wt");
        git(
            &main,
            &["worktree", "add", "-q", worktree.to_str().unwrap()],
        );

        let from_worktree = git_common_dir(&worktree).unwrap();

        assert_eq!(
            from_worktree.as_path().canonicalize().unwrap(),
            main.join(".git").canonicalize().unwrap()
        );
    }

    #[test]
    fn outside_a_repository_there_is_no_common_dir() {
        let dir = tempfile::tempdir().unwrap();

        assert!(git_common_dir(dir.path()).is_err());
    }
}
