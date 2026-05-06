use anyhow::{bail, Context, Result};
use std::path::Path;

pub(crate) mod ci;

pub(crate) fn launch_in_tmux_split(command: &str, cwd: &Path) -> Result<()> {
    if std::env::var("TMUX").is_err() {
        bail!("not in tmux; investigation requires a tmux session");
    }

    let status = std::process::Command::new("tmux")
        .args(["split-window", "-h", "-c"])
        .arg(cwd)
        .arg(command)
        .status()
        .context("failed to start tmux split-window")?;

    if !status.success() {
        bail!("tmux split-window failed with {status}");
    }

    Ok(())
}
