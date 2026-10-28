//! Every tmux invocation hub makes.

use super::window::OpenWindow;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// Windows open in the current tmux session.
///
/// Scoped to the current session, which is `list-windows`' default. A window in
/// another session could not be brought into view by `select-window` anyway, and
/// hub's TUI and its investigations share one session.
pub(super) fn open_windows() -> Result<Vec<OpenWindow>> {
    let out = Command::new("tmux")
        .args(["list-windows", "-F", "#{window_id} #W"])
        .output()
        .context("failed to run tmux list-windows")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("tmux list-windows failed: {stderr}");
    }

    Ok(parse_open_windows(&String::from_utf8_lossy(&out.stdout)))
}

/// Moves the session to the window with this id.
///
/// Selected by id rather than by name. Window names are
/// `<project>:<kind>:<discriminator>`, and `:` is also how tmux separates
/// session from window in a target, so `-t media:pr:92` asks for window
/// `pr:92` of session `media` instead of the window actually called
/// `media:pr:92`. A window id never contains one.
pub(super) fn select_window(id: &str) -> Result<()> {
    let status = Command::new("tmux")
        .args(["select-window", "-t", id])
        .status()
        .context("failed to run tmux select-window")?;
    if !status.success() {
        bail!("tmux select-window failed for {id} with {status}");
    }
    Ok(())
}

/// Opens a window named `name` running `command`, and moves the session to it.
pub(super) fn new_window(
    name: &str,
    cwd: Option<&Path>,
    env: &[(String, String)],
    command: &str,
) -> Result<()> {
    let mut cmd = Command::new("tmux");
    let _ = cmd.args(["new-window", "-n", name]);
    if let Some(cwd) = cwd {
        let _ = cmd.arg("-c").arg(cwd);
    }
    for (key, value) in env {
        let _ = cmd.arg("-e").arg(format!("{key}={value}"));
    }
    let _ = cmd.arg(command);

    let status = cmd.status().context("failed to start tmux new-window")?;
    if !status.success() {
        bail!("tmux new-window failed for {name} with {status}");
    }
    Ok(())
}

/// Reads `tmux list-windows -F '#{window_id} #W'` output.
///
/// Split on the first space only, because a window hub did not open can have
/// spaces in its name. A line without one is not a window and is skipped.
fn parse_open_windows(listing: &str) -> Vec<OpenWindow> {
    listing
        .lines()
        .filter_map(|line| {
            let (id, name) = line.split_once(' ')?;
            Some(OpenWindow {
                id: id.to_string(),
                name: name.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_open_windows;

    #[test]
    fn a_listing_pairs_each_id_with_its_name() {
        let parsed = parse_open_windows("@0 nvim\n@7 hub:pr:330\n");
        let pairs: Vec<(String, String)> = parsed
            .into_iter()
            .map(|window| (window.id, window.name))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("@0".to_string(), "nvim".to_string()),
                ("@7".to_string(), "hub:pr:330".to_string()),
            ]
        );
    }

    /// tmux allows spaces in a window name, and only the id is delimited.
    #[test]
    fn a_window_name_containing_spaces_survives_parsing() {
        let parsed = parse_open_windows("@3 my long name\n");
        assert_eq!(parsed[0].name, "my long name");
    }

    #[test]
    fn a_line_without_a_delimiter_is_not_a_window() {
        assert!(parse_open_windows("garbage\n").is_empty());
    }

    #[test]
    fn an_empty_listing_parses_to_no_windows() {
        assert!(parse_open_windows("").is_empty());
    }
}
