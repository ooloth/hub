//! Every tmux invocation hub makes.

use anyhow::{bail, Context, Result};
use domain::InvestigationWindow;
use std::path::Path;
use std::process::Command;

/// The right to open one tmux window, given out only for a name tmux did not
/// already have.
///
/// `open` is a method on this type and nothing else constructs it, so "open a
/// window without having looked for it" is not expressible, and neither is
/// "shell out to tmux without having checked we are in tmux" — `claim` does
/// both checks before handing one back.
///
/// Vacant *when checked*. tmux permits duplicate window names and nothing stops
/// one appearing in between, so this type forces the check rather than proving
/// the result still holds. For a single-user TUI that race is not worth closing.
pub(crate) struct VacantWindow(InvestigationWindow);

/// What became of a request for a window.
pub(crate) enum WindowClaim {
    /// tmux already had the window, and the session is now looking at it.
    Reused,
    /// tmux did not have the window. Only this can be opened.
    Vacant(VacantWindow),
}

/// Switches to `window` if tmux already has it, or hands back the right to open
/// it.
///
/// A `Reused` claim means the caller is done: the user is looking at the window
/// they asked for, and every step the caller would otherwise take (fetching,
/// creating a worktree, writing a prompt) is skipped.
///
/// `purpose` names what the caller was trying to do, so a missing tmux stays
/// actionable: "opening lazygit requires a tmux session" tells the user more
/// than "not in tmux" alone.
pub(crate) fn claim(window: &InvestigationWindow, purpose: &str) -> Result<WindowClaim> {
    if std::env::var("TMUX").is_err() {
        bail!("not in tmux; {purpose} requires a tmux session");
    }

    let Some(id) = find_window(&open_windows()?, window) else {
        return Ok(WindowClaim::Vacant(VacantWindow(window.clone())));
    };

    // Selected by id rather than by name. Window names are
    // `<project>:<kind>:<discriminator>`, and `:` is also how tmux separates
    // session from window in a target, so `-t media:pr:92` asks for window
    // `pr:92` of session `media` instead of the window actually called
    // `media:pr:92`. A window id never contains one.
    let status = Command::new("tmux")
        .args(["select-window", "-t", &id])
        .status()
        .context("failed to run tmux select-window")?;
    if !status.success() {
        bail!("tmux select-window failed for {window} with {status}");
    }

    Ok(WindowClaim::Reused)
}

impl VacantWindow {
    /// Opens the window and runs `command` in it.
    ///
    /// The window is attached: tmux moves the session to it, which is what makes
    /// the keypress that asked for it feel like going somewhere. Taking `self`
    /// by value spends the claim, so one check cannot open two windows.
    pub(crate) fn open(
        self,
        cwd: Option<&Path>,
        env: &[(String, String)],
        command: &str,
    ) -> Result<()> {
        assert!(!command.is_empty(), "tmux window command must not be empty");

        let Self(window) = self;
        let name = window.to_string();

        let mut cmd = Command::new("tmux");
        let _ = cmd.args(["new-window", "-n", &name]);
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
}

/// A window tmux currently has open.
struct OpenWindow {
    /// tmux's own handle for the window, like `@7`. Stable while the window
    /// lives and, unlike the name, safe to use as a target.
    id: String,
    name: String,
}

/// Windows open in the current tmux session.
///
/// Scoped to the current session, which is `list-windows`' default. A window in
/// another session could not be brought into view by `select-window` anyway, and
/// hub's TUI and its investigations share one session.
fn open_windows() -> Result<Vec<OpenWindow>> {
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

/// The id of the window under this name, if tmux has one.
///
/// Comparison is on the whole name in both directions: a substring match would
/// let `hub:pr:33` find `hub:pr:330`, or the reverse, and hijacking another
/// investigation's window is worse than opening a second one.
fn find_window(open: &[OpenWindow], window: &InvestigationWindow) -> Option<String> {
    let target = window.to_string();
    open.iter()
        .find(|candidate| candidate.name == target)
        .map(|candidate| candidate.id.clone())
}

#[cfg(test)]
mod tests {
    use super::{find_window, parse_open_windows, OpenWindow};
    use domain::InvestigationWindow;
    use proptest::prelude::*;
    use rstest::rstest;

    /// Window ids are irrelevant to every test that only asks whether a name
    /// matched, so they are generated rather than spelled out.
    fn open(names: &[&str]) -> Vec<OpenWindow> {
        names
            .iter()
            .enumerate()
            .map(|(i, name)| OpenWindow {
                id: format!("@{i}"),
                name: (*name).to_string(),
            })
            .collect()
    }

    #[test]
    fn a_window_already_open_is_found_by_its_id() {
        let windows = open(&["nvim", "hub:pr:330", "shell"]);
        assert_eq!(
            find_window(&windows, &InvestigationWindow::pr("ooloth/hub", 330)),
            Some("@1".to_string())
        );
    }

    #[test]
    fn an_empty_window_list_reports_not_open() {
        assert_eq!(
            find_window(&[], &InvestigationWindow::pr("ooloth/hub", 330)),
            None
        );
    }

    #[test]
    fn an_unrelated_window_list_reports_not_open() {
        let windows = open(&["nvim", "claude.exe", "just-", "scripts:ci:lint"]);
        assert_eq!(
            find_window(&windows, &InvestigationWindow::pr("ooloth/hub", 330)),
            None
        );
    }

    /// The open window is longer or shorter than the one being looked for, so
    /// this fails against a substring match in either direction.
    #[rstest]
    #[case("hub:pr:33")]
    #[case("hub:pr:3300")]
    #[case("hub:pr:330:extra")]
    #[case("x-hub:pr:330")]
    fn a_neighbouring_window_name_is_not_a_match(#[case] open_name: &str) {
        let windows = open(&[open_name]);
        assert_eq!(
            find_window(&windows, &InvestigationWindow::pr("ooloth/hub", 330)),
            None
        );
    }

    /// A repository numbers its pull requests and issues from one sequence, so
    /// the kind segment has to count.
    #[test]
    fn an_issue_does_not_match_a_pull_request_with_the_same_number() {
        let windows = open(&[&InvestigationWindow::pr("ooloth/hub", 330).to_string()]);
        assert_eq!(
            find_window(&windows, &InvestigationWindow::issue("ooloth/hub", 330)),
            None
        );
    }

    /// The id is what `select-window` is given, so the wrong one switches to the
    /// wrong window.
    #[test]
    fn the_id_returned_belongs_to_the_matching_window() {
        let windows = vec![
            OpenWindow {
                id: "@4".to_string(),
                name: "hub:pr:12".to_string(),
            },
            OpenWindow {
                id: "@9".to_string(),
                name: "hub:pr:330".to_string(),
            },
        ];
        assert_eq!(
            find_window(&windows, &InvestigationWindow::pr("ooloth/hub", 330)),
            Some("@9".to_string())
        );
    }

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

    proptest! {
        /// Whatever else tmux happens to have open, the window hub is looking
        /// for is found when it is there.
        #[test]
        fn a_window_is_found_among_any_other_windows(
            repo in "[a-z][a-z-]{0,18}",
            number in 1u64..=100_000,
            others in prop::collection::vec("[a-z:0-9-]{0,20}", 0..6),
        ) {
            let window = InvestigationWindow::pr(&repo, number);
            let mut names: Vec<String> = others;
            names.push(window.to_string());
            let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
            prop_assert!(find_window(&open(&borrowed), &window).is_some());
        }

        /// The generator is biased toward one repository and adjacent numbers,
        /// because names sharing a prefix are where a substring bug hides.
        #[test]
        fn a_list_holding_one_window_does_not_report_another(
            repo in "hub(-[a-z]{1,4})?",
            a in 1u64..=400,
            b in 1u64..=400,
        ) {
            prop_assume!(a != b);
            let present = InvestigationWindow::pr(&repo, a).to_string();
            prop_assert!(
                find_window(&open(&[&present]), &InvestigationWindow::pr(&repo, b)).is_none()
            );
        }
    }
}
