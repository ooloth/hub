//! Every Rex invocation hub makes.
//!
//! Rex calls a window a tab, and its CLI calls it a window. Every call names
//! the session explicitly, so the TUI's own session is the one acted on even
//! when the user is looking at another.

use super::window::OpenWindow;
use anyhow::{bail, Context, Result};
use domain::RexSessionId;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

/// Sets the variables and then runs the shell, so each value reaches the
/// process as its own argv word and no shell parses it on the way.
///
/// Rex has no option for a terminal's environment, which is what `tmux -e`
/// gives the tmux path. Absolute, so the program is the same whatever PATH the
/// login shell sets.
const ENV: &str = "/usr/bin/env";

/// Runs hub's command. A fixed POSIX shell rather than the user's, so one
/// command means the same on every machine; hub writes every command it runs.
const SHELL: &str = "/bin/sh";

/// Windows open in the session.
pub(super) fn open_windows(session: &RexSessionId) -> Result<Vec<OpenWindow>> {
    let out = run(&[
        "window".into(),
        "ls".into(),
        "-s".into(),
        session.as_str().into(),
        "--json".into(),
    ])
    .context("rex window ls failed")?;
    parse_window_listing(&out)
}

/// Makes the window with this id the session's active one.
///
/// Focused by id rather than by label, because a label can match more than
/// one window and an id cannot.
pub(super) fn focus_window(session: &RexSessionId, id: &str) -> Result<()> {
    let _ = run(&[
        "window".into(),
        "focus".into(),
        "-s".into(),
        session.as_str().into(),
        id.into(),
    ])
    .with_context(|| format!("rex window focus failed for {id}"))?;
    Ok(())
}

/// Opens a window labelled `name` running `command`, and makes it active.
pub(super) fn new_window(
    session: &RexSessionId,
    name: &str,
    cwd: Option<&Path>,
    env: &[(String, String)],
    command: &str,
) -> Result<()> {
    let _ = run(&new_window_args(session, name, cwd, env, command))
        .with_context(|| format!("rex window new failed for {name}"))?;
    Ok(())
}

/// Runs `rex` with `args`, returning what it printed.
///
/// Output is captured rather than inherited, because the TUI owns the screen
/// and anything `rex` printed would land on top of it. A failure carries
/// `rex`'s stderr and never `args`, which hold the prompts and MCP config.
fn run(args: &[OsString]) -> Result<String> {
    let out = Command::new("rex")
        .args(args)
        .output()
        .context("failed to run rex")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("{}: {}", out.status, stderr.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The arguments to `rex` that open one window.
///
/// # Panics
/// Panics if `name` starts with `-`, which `rex` would read as an option, if
/// `command` is empty, or if an environment key is empty, contains `=`, or
/// starts with `-`, any of which `env` would misread.
fn new_window_args(
    session: &RexSessionId,
    name: &str,
    cwd: Option<&Path>,
    env: &[(String, String)],
    command: &str,
) -> Vec<OsString> {
    assert!(
        !name.starts_with('-'),
        "window name must not start with '-', got {name}"
    );
    assert!(!command.is_empty(), "window command must not be empty");
    for (key, _) in env {
        assert!(!key.is_empty(), "environment key must not be empty");
        assert!(
            !key.contains('='),
            "environment key must not contain '=', got {key}"
        );
        assert!(
            !key.starts_with('-'),
            "environment key must not start with '-', got {key}"
        );
    }

    let mut args: Vec<OsString> = vec![
        "window".into(),
        "new".into(),
        "-s".into(),
        session.as_str().into(),
        name.into(),
    ];
    if let Some(cwd) = cwd {
        args.push("-c".into());
        args.push(cwd.into());
    }
    args.push("--".into());
    args.push(ENV.into());
    args.extend(
        env.iter()
            .map(|(key, value)| OsString::from(format!("{key}={value}"))),
    );
    args.push(SHELL.into());
    args.push("-c".into());
    args.push(command.into());
    args
}

/// Reads `rex window ls --json` output.
///
/// A window without a label cannot be one hub opened, so it is skipped rather
/// than rejected.
///
/// # Errors
/// Returns an error when the output is not a listing of windows with ids.
fn parse_window_listing(listing: &str) -> Result<Vec<OpenWindow>> {
    let parsed: serde_json::Value =
        serde_json::from_str(listing).context("rex window ls printed something other than JSON")?;
    let Some(windows) = parsed.get("windows").and_then(serde_json::Value::as_array) else {
        bail!("rex window ls printed no windows array");
    };

    let mut open = Vec::with_capacity(windows.len());
    for window in windows {
        let Some(id) = window.get("window_id").and_then(serde_json::Value::as_str) else {
            bail!("rex window ls printed a window without a window_id");
        };
        let Some(name) = window.get("label").and_then(serde_json::Value::as_str) else {
            continue;
        };
        open.push(OpenWindow {
            id: id.to_string(),
            name: name.to_string(),
        });
    }
    Ok(open)
}

#[cfg(test)]
mod tests {
    use super::{new_window_args, parse_window_listing, ENV, SHELL};
    use domain::RexSessionId;
    use proptest::prelude::*;
    use std::ffi::OsString;
    use std::path::Path;
    use std::process::Command;

    const SESSION: &str = "session:01a17aa1-051b-7b98-a328-ba981c77fd28";

    fn session() -> RexSessionId {
        RexSessionId::parse(SESSION).unwrap()
    }

    /// Recorded from `rex window ls -s SESSION --json`, trimmed to the fields
    /// hub reads plus enough of the rest to show they are ignored.
    const LISTING: &str = r#"{
      "revision": 5,
      "session_id": "session:01a17aa1-051b-7b98-a328-ba981c77fd28",
      "label": "hub-spike",
      "active_window_id": "window:01a17aa1-0529-78e9-8ef0-86ad5a0faf16",
      "windows": [
        {
          "window_id": "window:01a17aa1-0529-78e9-8ef0-86ad5a0faf16",
          "label": "Window 5a0faf16",
          "focused_block_id": "block:01a17aa1-0529-78e9-8ef0-86ad5a0faf17",
          "layers": []
        },
        {
          "window_id": "window:01a17aa1-6c67-79c0-8759-16b8d8c0997d",
          "label": "hub:pr:92",
          "layers": []
        }
      ]
    }"#;

    fn pairs(listing: &str) -> Vec<(String, String)> {
        parse_window_listing(listing)
            .unwrap()
            .into_iter()
            .map(|window| (window.id, window.name))
            .collect()
    }

    #[test]
    fn a_listing_pairs_each_window_id_with_its_label() {
        assert_eq!(
            pairs(LISTING),
            vec![
                (
                    "window:01a17aa1-0529-78e9-8ef0-86ad5a0faf16".to_string(),
                    "Window 5a0faf16".to_string()
                ),
                (
                    "window:01a17aa1-6c67-79c0-8759-16b8d8c0997d".to_string(),
                    "hub:pr:92".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_label_containing_spaces_and_colons_survives_parsing() {
        let listing = r#"{"windows":[{"window_id":"window:a","label":"my long: name"}]}"#;
        assert_eq!(
            pairs(listing),
            vec![("window:a".to_string(), "my long: name".to_string())]
        );
    }

    #[test]
    fn a_window_without_a_label_is_skipped() {
        let listing =
            r#"{"windows":[{"window_id":"window:a"},{"window_id":"window:b","label":"hub:pr:1"}]}"#;
        assert_eq!(
            pairs(listing),
            vec![("window:b".to_string(), "hub:pr:1".to_string())]
        );
    }

    #[test]
    fn a_session_with_no_windows_parses_to_none() {
        assert!(pairs(r#"{"windows":[]}"#).is_empty());
    }

    #[test]
    fn output_that_is_not_json_is_an_error() {
        assert!(parse_window_listing("Error: session not found").is_err());
    }

    #[test]
    fn a_listing_without_windows_is_an_error() {
        assert!(parse_window_listing(r#"{"revision":5}"#).is_err());
    }

    #[test]
    fn a_window_without_an_id_is_an_error() {
        assert!(parse_window_listing(r#"{"windows":[{"label":"hub:pr:1"}]}"#).is_err());
    }

    #[test]
    fn the_arguments_open_a_labelled_window_in_the_session_running_the_command() {
        let args = new_window_args(
            &session(),
            "hub:pr:92",
            Some(Path::new("/tmp/worktree")),
            &[("HUB_SYSTEM_PROMPT".to_string(), "be careful".to_string())],
            "claude \"$HUB_SYSTEM_PROMPT\"",
        );
        let expected: Vec<OsString> = [
            "window",
            "new",
            "-s",
            SESSION,
            "hub:pr:92",
            "-c",
            "/tmp/worktree",
            "--",
            ENV,
            "HUB_SYSTEM_PROMPT=be careful",
            SHELL,
            "-c",
            "claude \"$HUB_SYSTEM_PROMPT\"",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        assert_eq!(args, expected);
    }

    #[test]
    fn without_a_directory_the_window_starts_where_rex_chooses() {
        let args = new_window_args(&session(), "hub:diff:1", None, &[], "gh pr diff 1");
        let expected: Vec<OsString> = [
            "window",
            "new",
            "-s",
            SESSION,
            "hub:diff:1",
            "--",
            ENV,
            SHELL,
            "-c",
            "gh pr diff 1",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        assert_eq!(args, expected);
    }

    #[test]
    #[should_panic(expected = "must not contain '='")]
    fn an_environment_key_containing_equals_is_refused() {
        let _ = new_window_args(
            &session(),
            "hub:pr:1",
            None,
            &[("A=B".to_string(), "C".to_string())],
            "true",
        );
    }

    /// `InvestigationWindow` trims leading dashes, so only a bug reaches this,
    /// and `rex` would read the name as an option.
    #[test]
    #[should_panic(expected = "must not start with '-'")]
    fn a_window_name_starting_with_a_dash_is_refused() {
        let _ = new_window_args(&session(), "-hub:pr:1", None, &[], "true");
    }

    #[test]
    #[should_panic(expected = "must not be empty")]
    fn an_empty_command_is_refused() {
        let _ = new_window_args(&session(), "hub:pr:1", None, &[], "");
    }

    /// Runs the part of the arguments Rex hands to the new terminal, here, and
    /// returns the value each variable held, in order.
    fn values_seen_by_the_command(env: &[(String, String)]) -> Vec<String> {
        let print = env
            .iter()
            .map(|(key, _)| format!("\"${key}\""))
            .collect::<Vec<_>>()
            .join(" ");
        let command = format!("printf '%s\\0' {print}");
        let args = new_window_args(&session(), "hub:pr:1", None, env, &command);
        let tail = &args[args.iter().position(|arg| arg == "--").unwrap() + 1..];
        let out = Command::new(&tail[0]).args(&tail[1..]).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout)
            .unwrap()
            .split_terminator('\0')
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn text_a_shell_would_interpret_reaches_the_command_unchanged() {
        let tricky = "line1\n'single' \"double\" $HOME `whoami` $(echo hi) \\ ; && | > * ~ !\r\n\tdone ✓ 日本 🙂 a=b=c";
        let env = vec![("HUB_TASK_PROMPT".to_string(), tricky.to_string())];
        assert_eq!(values_seen_by_the_command(&env), vec![tricky.to_string()]);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Whatever a prompt holds, the command sees exactly that text. NUL is
        /// excluded because no process environment can hold it.
        #[test]
        fn every_value_reaches_the_command_byte_for_byte(
            values in prop::collection::vec("[^\\x00]{0,300}", 1..4),
        ) {
            let env: Vec<(String, String)> = values
                .iter()
                .enumerate()
                .map(|(i, value)| (format!("HUB_VALUE_{i}"), value.clone()))
                .collect();
            prop_assert_eq!(values_seen_by_the_command(&env), values);
        }
    }
}
