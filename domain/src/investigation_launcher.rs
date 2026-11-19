//! The program every investigation starts in place of `claude`.
//!
//! Chosen per device in `hub.toml`, as
//! docs/decisions/034-investigations-start-through-the-launcher-hub-toml-names.md
//! records.

use std::path::Path;

/// What an investigation's window runs, given the arguments hub would pass
/// `claude`. A launcher is expected to pass them on to `claude`, and hub does
/// not check that it does (decision 034).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum InvestigationLauncher {
    /// `claude`, found on the window's `PATH`.
    #[default]
    Claude,
    /// An executable named by absolute path, such as a launcher that sets up
    /// the device's logins and Vertex AI project before starting `claude`.
    /// Nothing here checks the file exists or can run: the TUI checks that
    /// when it starts and at each launch.
    Executable(LauncherPath),
}

/// An absolute path, which only [`InvestigationLauncher::executable`] builds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LauncherPath(String);

impl InvestigationLauncher {
    /// # Errors
    /// Returns an error naming `path` if it is empty or not absolute, since a
    /// path with a slash would resolve against each investigation's worktree,
    /// and a bare name against the window's `PATH`.
    pub fn executable(path: &str) -> Result<Self, String> {
        if Path::new(path).is_absolute() {
            Ok(Self::Executable(LauncherPath(path.to_string())))
        } else {
            Err(format!(
                "launcher {path:?} is not an absolute path to an executable"
            ))
        }
    }

    /// The program a window runs, which reaches the window as the value of
    /// `HUB_LAUNCHER` rather than as text in its command.
    #[must_use]
    pub fn program(&self) -> &str {
        match self {
            Self::Claude => "claude",
            Self::Executable(LauncherPath(path)) => path,
        }
    }

    /// The file to check can run, or `None` for `claude`, which is looked up
    /// on the window's `PATH` when the window opens.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Claude => None,
            Self::Executable(LauncherPath(path)) => Some(Path::new(path)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn the_default_is_claude() {
        assert_eq!(InvestigationLauncher::default().program(), "claude");
        assert_eq!(InvestigationLauncher::default().path(), None);
    }

    #[test]
    fn an_absolute_path_is_the_program() {
        let launcher = InvestigationLauncher::executable("/opt/my tools/cc.bash").unwrap();
        assert_eq!(launcher.program(), "/opt/my tools/cc.bash");
        assert_eq!(launcher.path(), Some(Path::new("/opt/my tools/cc.bash")));
    }

    #[rstest]
    #[case::empty("")]
    #[case::a_bare_name("cc.bash")]
    #[case::a_relative_path("tools/claude/cc.bash")]
    #[case::a_home_relative_path("~/Repos/ooloth/dotfiles/tools/claude/cc.bash")]
    fn a_path_that_is_not_absolute_is_rejected_naming_it(#[case] path: &str) {
        let err = InvestigationLauncher::executable(path).unwrap_err();
        assert!(err.contains(&format!("{path:?}")), "{err}");
    }
}
