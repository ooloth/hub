use anyhow::{bail, Context, Result};
use std::path::Path;

pub(crate) mod ci;
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

pub(crate) fn launch(config: LaunchConfig, cwd: &Path) -> Result<()> {
    if std::env::var("TMUX").is_err() {
        bail!("not in tmux; investigation requires a tmux session");
    }

    let task_arg = if config.prompt.is_empty() {
        String::new()
    } else {
        " \"$HUB_TASK_PROMPT\"".to_string()
    };

    let command = format!(
        "claude --dangerously-skip-permissions --model {} --allowedTools '{}' --system-prompt \"$HUB_SYSTEM_PROMPT\"{}",
        config.model,
        config.allowed_tools,
        task_arg,
    );

    let pane = std::env::var("TMUX_PANE").unwrap_or_default();

    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["split-window", "-h", "-t", &pane, "-c"]).arg(cwd);
    cmd.arg("-e")
        .arg(format!("HUB_SYSTEM_PROMPT={}", config.system_prompt));
    cmd.arg("-e")
        .arg(format!("HUB_TASK_PROMPT={}", config.prompt));
    cmd.arg("-e").arg("GIT_TERMINAL_PROMPT=0");
    cmd.arg("-e")
        .arg("GIT_CONFIG_PARAMETERS=credential.helper=!gh auth git-credential");
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
