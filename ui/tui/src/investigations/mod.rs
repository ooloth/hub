use anyhow::{bail, Context, Result};
use std::path::Path;

pub(crate) mod ci;

pub(crate) struct LaunchConfig {
    pub(crate) command: String,
}

pub(crate) fn launch(config: LaunchConfig, cwd: &Path) -> Result<()> {
    if std::env::var("TMUX").is_err() {
        bail!("not in tmux; investigation requires a tmux session");
    }

    let status = std::process::Command::new("tmux")
        .args(["split-window", "-h", "-c"])
        .arg(cwd)
        .arg(&config.command)
        .status()
        .context("failed to start tmux split-window")?;

    if !status.success() {
        bail!("tmux split-window failed with {status}");
    }

    Ok(())
}
