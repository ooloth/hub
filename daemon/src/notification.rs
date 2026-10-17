//! Posting a macOS notification through `osascript`.
use std::time::Duration;

use anyhow::{Context, Result};
use domain::credential_notification::Notification;

/// How long `osascript` may take before posting counts as failed.
const POST_DEADLINE: Duration = Duration::from_secs(10);

/// The script reads the title and body from its arguments, so no text can change what it does.
const SCRIPT: [&str; 6] = [
    "-e",
    "on run argv",
    "-e",
    "display notification (item 2 of argv) with title (item 1 of argv)",
    "-e",
    "end run",
];

fn post_command(notification: &Notification) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("osascript");
    let _ = command
        .args(SCRIPT)
        .arg(&notification.title)
        .arg(&notification.body)
        .kill_on_drop(true);
    command
}

/// Posts `notification`, or says why it could not.
///
/// # Errors
/// Returns an error when `osascript` cannot be run, takes longer than [`POST_DEADLINE`], or exits
/// non-zero.
pub(crate) async fn post(notification: Notification) -> Result<()> {
    let output = tokio::time::timeout(POST_DEADLINE, post_command(&notification).output())
        .await
        .context("posting the notification took too long")?
        .context("failed to run osascript")?;
    anyhow::ensure!(
        output.status.success(),
        "osascript exited {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn the_title_and_body_reach_osascript_unchanged_as_arguments(
            title in "[^\u{0}]{0,60}",
            body in "[^\u{0}]{0,200}",
        ) {
            let notification = Notification { title: title.clone(), body: body.clone() };
            let command = post_command(&notification);
            let args: Vec<String> = command
                .as_std()
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();

            prop_assert_eq!(command.as_std().get_program(), "osascript");
            prop_assert_eq!(&args[..6], &SCRIPT.map(str::to_string)[..]);
            prop_assert_eq!(&args[6..], &[title, body][..]);
        }
    }
}
