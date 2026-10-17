//! Posting a macOS notification through `terminal-notifier`, and removing it again.
//!
//! `terminal-notifier` notifications stay on screen until dismissed, and every one hub posts is in
//! one group, so each replaces the last and all of them can be removed at once. Measured under
//! launchd on 2026-10-17: `osascript` notifications from the daemon never reached the screen.
use std::time::Duration;

use anyhow::{Context, Result};
use domain::credential_notification::Notification;

/// How long `terminal-notifier` may take before posting or removing counts as failed. Its
/// first-run permission prompt once blocked it for over two minutes.
const DEADLINE: Duration = Duration::from_secs(10);

/// The group every notification about missing credentials is posted in.
const GROUP: &str = "hub-credentials";

fn post_command(notification: &Notification) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("terminal-notifier");
    let _ = command
        .arg("-title")
        .arg(&notification.title)
        .arg("-message")
        .arg(&notification.body)
        .args(["-group", GROUP])
        .kill_on_drop(true);
    command
}

fn remove_command() -> tokio::process::Command {
    let mut command = tokio::process::Command::new("terminal-notifier");
    let _ = command.args(["-remove", GROUP]).kill_on_drop(true);
    command
}

async fn run(mut command: tokio::process::Command, what: &str) -> Result<()> {
    let output = tokio::time::timeout(DEADLINE, command.output())
        .await
        .with_context(|| format!("{what} took too long"))?
        .context("failed to run terminal-notifier")?;
    anyhow::ensure!(
        output.status.success(),
        "terminal-notifier exited {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

/// Posts `notification`, replacing any earlier one hub posted, or says why it could not.
///
/// # Errors
/// Returns an error when `terminal-notifier` cannot be run, takes longer than [`DEADLINE`], or
/// exits non-zero.
pub(crate) async fn post(notification: Notification) -> Result<()> {
    run(post_command(&notification), "posting the notification").await
}

/// Removes every notification hub posted about missing credentials.
///
/// # Errors
/// As [`post`].
pub(crate) async fn remove() -> Result<()> {
    run(remove_command(), "removing the notification").await
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn args(command: &tokio::process::Command) -> Vec<String> {
        command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    proptest! {
        #[test]
        fn the_title_and_body_reach_terminal_notifier_unchanged_in_hubs_group(
            title in "[^\u{0}]{0,60}",
            body in "[^\u{0}]{0,200}",
        ) {
            let command = post_command(&Notification { title: title.clone(), body: body.clone() });

            prop_assert_eq!(command.as_std().get_program(), "terminal-notifier");
            prop_assert_eq!(
                args(&command),
                vec!["-title".to_string(), title, "-message".to_string(), body,
                     "-group".to_string(), GROUP.to_string()]
            );
        }
    }

    #[test]
    fn removing_clears_hubs_group() {
        let command = remove_command();

        assert_eq!(command.as_std().get_program(), "terminal-notifier");
        assert_eq!(args(&command), vec!["-remove", GROUP]);
    }
}
