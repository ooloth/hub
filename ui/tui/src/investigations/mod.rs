use anyhow::{bail, Context, Result};
use std::path::Path;

pub(crate) mod ci;
pub(crate) mod loki;
#[cfg(feature = "private")]
pub(crate) mod media;

pub(crate) struct LaunchConfig {
    pub(crate) command: String,
    pub(crate) env: Vec<(String, String)>,
}

pub(crate) fn launch(config: LaunchConfig, cwd: &Path) -> Result<()> {
    if std::env::var("TMUX").is_err() {
        bail!("not in tmux; investigation requires a tmux session");
    }

    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["split-window", "-h", "-c"]).arg(cwd);
    for (k, v) in &config.env {
        cmd.arg("-e").arg(format!("{k}={v}"));
    }
    cmd.arg(&config.command);

    let status = cmd.status().context("failed to start tmux split-window")?;

    if !status.success() {
        bail!("tmux split-window failed with {status}");
    }

    Ok(())
}
