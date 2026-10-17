//! Posting a macOS notification through `terminal-notifier`, and removing it again.
//!
//! `terminal-notifier` notifications stay on screen until dismissed. Every notification about
//! missing credentials is in one group, so each replaces the last and all of them can be removed at
//! once. Each pull request's card is in a group of its own. Measured under launchd on 2026-10-17:
//! `osascript` notifications from the daemon never reached the screen.
use std::time::Duration;

use anyhow::{Context, Result};
use domain::credential_notification::Notification;
use domain::pr_turn::TurnCard;

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

/// `-message`, `-title` and `-subtitle` values are read as text only when they do not start with
/// one of these. Measured 2026-10-17: a message starting with `[` made terminal-notifier 3.1.0 exit
/// 2 with "Could not read the -message value", and `\[` showed as `[`. Its error names the others.
const NEEDS_ESCAPING: [char; 5] = ['[', '(', '{', '"', '\''];

/// `text` as terminal-notifier will show it unchanged.
fn shown_as_written(text: &str) -> String {
    match text.chars().next() {
        Some(first) if NEEDS_ESCAPING.contains(&first) => format!("\\{text}"),
        _ => text.to_string(),
    }
}

fn post_turn_command(group: &str, card: &TurnCard) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("terminal-notifier");
    let _ = command
        .arg("-title")
        .arg(shown_as_written(&card.title))
        .arg("-subtitle")
        .arg(shown_as_written(&card.subtitle))
        .arg("-message")
        .arg(shown_as_written(card.message.expose()))
        .arg("-open")
        .arg(&card.url)
        .arg("-group")
        .arg(group)
        .kill_on_drop(true);
    command
}

fn remove_group_command(group: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("terminal-notifier");
    let _ = command.arg("-remove").arg(group).kill_on_drop(true);
    command
}

fn list_command() -> tokio::process::Command {
    let mut command = tokio::process::Command::new("terminal-notifier");
    let _ = command.args(["-list", "ALL"]).kill_on_drop(true);
    command
}

async fn run(command: tokio::process::Command, what: &str) -> Result<()> {
    let _ = output_of(command, what).await?;
    Ok(())
}

/// What `command` printed, once it has exited successfully within [`DEADLINE`].
async fn output_of(mut command: tokio::process::Command, what: &str) -> Result<String> {
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
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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

/// Posts `card` in `group`, replacing any card already there.
///
/// # Errors
/// As [`post`].
pub(crate) async fn post_turn(group: String, card: TurnCard) -> Result<()> {
    run(post_turn_command(&group, &card), "posting the card").await
}

/// Removes the card in `group`.
///
/// # Errors
/// As [`post`].
pub(crate) async fn remove_turn(group: String) -> Result<()> {
    run(remove_group_command(&group), "removing the card").await
}

/// What `terminal-notifier -list ALL` prints: every card showing, one line each after a header.
///
/// # Errors
/// As [`post`].
pub(crate) async fn list_shown() -> Result<String> {
    output_of(list_command(), "listing the cards showing").await
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

    fn card(title: &str, subtitle: &str, message: &str) -> TurnCard {
        TurnCard {
            title: title.to_string(),
            subtitle: subtitle.to_string(),
            message: domain::UntrustedText::new(message),
            url: "https://github.com/ooloth/hub/pull/344".to_string(),
        }
    }

    proptest! {
        #[test]
        fn a_card_reaches_terminal_notifier_as_written_in_its_own_group(
            message in r#"[^\x00\[({"'][^\x00]{0,200}"#,
            group in "hub-(default|dev)-pr-[a-z]{1,10}/[a-z]{1,10}#[0-9]{1,6}",
        ) {
            let command = post_turn_command(&group, &card("PR review requested", "hub #344 · @someone", &message));

            prop_assert_eq!(command.as_std().get_program(), "terminal-notifier");
            prop_assert_eq!(
                args(&command),
                vec!["-title".to_string(), "PR review requested".to_string(),
                     "-subtitle".to_string(), "hub #344 · @someone".to_string(),
                     "-message".to_string(), message,
                     "-open".to_string(), "https://github.com/ooloth/hub/pull/344".to_string(),
                     "-group".to_string(), group]
            );
        }
    }

    #[rstest::rstest]
    #[case::bracket("[WIP] a title", "\\[WIP] a title")]
    #[case::parenthesis("(maybe) a title", "\\(maybe) a title")]
    #[case::brace("{x} a title", "\\{x} a title")]
    #[case::double_quote("\"quoted\" title", "\\\"quoted\" title")]
    #[case::single_quote("'quoted' title", "\\'quoted' title")]
    #[case::later_bracket("a [WIP] title", "a [WIP] title")]
    #[case::empty("", "")]
    fn a_title_terminal_notifier_would_misread_is_escaped(
        #[case] message: &str,
        #[case] sent: &str,
    ) {
        let command = post_turn_command("g", &card(message, message, message));

        let sent_args = args(&command);
        assert_eq!(sent_args[1], sent, "title");
        assert_eq!(sent_args[3], sent, "subtitle");
        assert_eq!(sent_args[5], sent, "message");
    }

    #[test]
    fn removing_a_card_clears_only_its_group() {
        let command = remove_group_command("hub-dev-pr-ooloth/hub#344");

        assert_eq!(args(&command), vec!["-remove", "hub-dev-pr-ooloth/hub#344"]);
    }

    #[test]
    fn listing_asks_for_every_card_showing() {
        assert_eq!(args(&list_command()), vec!["-list", "ALL"]);
    }

    #[test]
    fn removing_clears_hubs_group() {
        let command = remove_command();

        assert_eq!(command.as_std().get_program(), "terminal-notifier");
        assert_eq!(args(&command), vec!["-remove", GROUP]);
    }
}
