//! Claiming a window by name: switching to it if it is open, or the right to open it.

use super::detect::Terminal;
use super::{rex, tmux};
use anyhow::Result;
use domain::InvestigationWindow;
use std::path::Path;

/// The right to open one window, given out only for a name the terminal did
/// not already have.
///
/// `open` is a method on this type and nothing else constructs it, so "open a
/// window without having looked for it" is not expressible, and neither is
/// "open a window in a terminal other than the one searched": the terminal
/// `claim` searched travels inside the claim.
///
/// Vacant *when checked*. tmux and Rex both permit duplicate window names and
/// nothing stops one appearing in between, so this type forces the check rather
/// than proving the result still holds. For a single-user TUI that race is not
/// worth closing.
pub(crate) struct VacantWindow {
    terminal: Terminal,
    window: InvestigationWindow,
}

/// What became of a request for a window.
pub(crate) enum WindowClaim {
    /// The terminal already had the window, and it is now in view.
    Reused,
    /// The terminal did not have the window. Only this can be opened.
    Vacant(VacantWindow),
}

/// Switches to `window` if the enclosing terminal already has it, or hands back
/// the right to open it.
///
/// A `Reused` claim means the caller is done: the user is looking at the window
/// they asked for, and every step the caller would otherwise take (fetching,
/// creating a worktree, writing a prompt) is skipped.
///
/// `purpose` names what the caller was trying to do, so a missing terminal
/// stays actionable.
pub(crate) fn claim(window: &InvestigationWindow, purpose: &str) -> Result<WindowClaim> {
    let terminal = Terminal::detect(
        std::env::var("TMUX").ok().as_deref(),
        std::env::var("REX_SESSION").ok().as_deref(),
        purpose,
    )?;

    let open = match &terminal {
        Terminal::Tmux => tmux::open_windows()?,
        Terminal::Rex { session } => rex::open_windows(session)?,
    };
    let Some(id) = find_window(&open, window) else {
        return Ok(WindowClaim::Vacant(VacantWindow {
            terminal,
            window: window.clone(),
        }));
    };

    match &terminal {
        Terminal::Tmux => tmux::select_window(&id)?,
        Terminal::Rex { session } => rex::focus_window(session, &id)?,
    }
    Ok(WindowClaim::Reused)
}

impl VacantWindow {
    /// Opens the window and runs `command` in it.
    ///
    /// The window becomes the one in view, which is what makes the keypress
    /// that asked for it feel like going somewhere. Taking `self` by value
    /// spends the claim, so one check cannot open two windows.
    pub(crate) fn open(
        self,
        cwd: Option<&Path>,
        env: &[(String, String)],
        command: &str,
    ) -> Result<()> {
        assert!(!command.is_empty(), "window command must not be empty");

        let Self { terminal, window } = self;
        let name = window.to_string();
        match &terminal {
            Terminal::Tmux => tmux::new_window(&name, cwd, env, command),
            Terminal::Rex { session } => rex::new_window(session, &name, cwd, env, command),
        }
    }
}

/// A window the terminal currently has open.
pub(super) struct OpenWindow {
    /// The terminal's own handle for the window, like tmux's `@7` or Rex's
    /// `window:<uuid>`. Stable while the window lives and, unlike the name,
    /// safe to use as a target.
    pub(super) id: String,
    pub(super) name: String,
}

/// The id of the window under this name, if the terminal has one.
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
    use super::{find_window, OpenWindow};
    use domain::InvestigationWindow;
    use proptest::prelude::*;
    use rstest::rstest;

    fn hub() -> domain::ProjectShortName {
        short("hub")
    }

    fn short(name: &str) -> domain::ProjectShortName {
        domain::ProjectShortName::parse(name).unwrap()
    }

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
            find_window(&windows, &InvestigationWindow::pr(&hub(), 330)),
            Some("@1".to_string())
        );
    }

    #[test]
    fn an_empty_window_list_reports_not_open() {
        assert_eq!(
            find_window(&[], &InvestigationWindow::pr(&hub(), 330)),
            None
        );
    }

    #[test]
    fn an_unrelated_window_list_reports_not_open() {
        let windows = open(&["nvim", "claude.exe", "just-", "scripts:ci:lint"]);
        assert_eq!(
            find_window(&windows, &InvestigationWindow::pr(&hub(), 330)),
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
            find_window(&windows, &InvestigationWindow::pr(&hub(), 330)),
            None
        );
    }

    /// A pull request's lazygit window shares its project and number with the
    /// investigation, so only the kind segment stops a lookup switching to it.
    #[test]
    fn a_pull_requests_lazygit_window_is_not_its_investigation_window() {
        let windows = open(&[&InvestigationWindow::lazygit(&hub(), 330).to_string()]);
        assert_eq!(
            find_window(&windows, &InvestigationWindow::pr(&hub(), 330)),
            None
        );
    }

    /// The id is what the terminal is told to switch to, so the wrong one switches to the
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
            find_window(&windows, &InvestigationWindow::pr(&hub(), 330)),
            Some("@9".to_string())
        );
    }

    proptest! {
        /// Whatever else the terminal happens to have open, the window hub is looking
        /// for is found when it is there.
        #[test]
        fn a_window_is_found_among_any_other_windows(
            repo in "[a-z]{1,8}(-[a-z]{1,4}){0,2}",
            number in 1u64..=100_000,
            others in prop::collection::vec("[a-z:0-9-]{0,20}", 0..6),
        ) {
            let window = InvestigationWindow::pr(&short(&repo), number);
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
            let present = InvestigationWindow::pr(&short(&repo), a).to_string();
            prop_assert!(
                find_window(&open(&[&present]), &InvestigationWindow::pr(&short(&repo), b)).is_none()
            );
        }
    }
}
