//! What a `LaunchAgent` for `hub-daemon` needs to know about this machine, resolved once.
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use domain::profile::Profile;

/// A path launchd can use as written: absolute and UTF-8. launchd expands neither `~` nor
/// variables, and a plist holds strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AbsolutePath(String);

impl AbsolutePath {
    /// # Errors
    /// Returns an error naming `path` when it is relative or not UTF-8.
    pub(crate) fn new(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            bail!(
                "{} is relative, and launchd runs the daemon from /",
                path.display()
            );
        }
        let text = path.to_str().with_context(|| {
            format!("{} is not UTF-8, which a plist cannot hold", path.display())
        })?;
        Ok(Self(text.to_string()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}

/// The `PATH` of the shell that ran the command: where `hub-daemon` and the daemon's tools are
/// found. It is not handed to the daemon, because it can hold per-session directories, such as
/// fnm's, that differ in every terminal and vanish with their shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CallerPath(String);

impl CallerPath {
    /// # Errors
    /// Returns an error when `PATH` is unset or empty.
    pub(crate) fn new(raw: Option<String>) -> Result<Self> {
        match raw {
            Some(path) if !path.is_empty() => Ok(Self(path)),
            _ => bail!("PATH is unset or empty, so neither hub-daemon nor its tools can be found"),
        }
    }

    /// The directories launchd could use as written, in order, without repeats.
    fn absolute_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for dir in self.0.split(':').filter_map(absolute_dir) {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        dirs
    }
}

/// Every program the daemon's code runs by name. The daemon's `PATH` holds where these are and
/// nothing else of the caller's. `daemon_tools_are_every_program_the_daemon_runs` holds this
/// list equal to the programs the code spawns.
pub(crate) const DAEMON_TOOLS: [&str; 3] = ["git", "op", "gcloud"];

/// Where launchd's own `PATH` already looks. The daemon's `PATH` ends with these.
const SYSTEM_DIRS: [&str; 4] = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"];

/// The daemon's `PATH`: the directories holding its tools, then the system directories.
/// Stable across terminals, and different only when a tool moves, which is when the daemon
/// should be reloaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchPath(pub(super) String);

impl SearchPath {
    /// Each tool's directory is where the caller's shell would find it, so the daemon runs the
    /// same `op` and `git` the shell does. A tool not on `caller` is left out: `gcloud` is
    /// needed only where a gcp-logs workflow is configured.
    pub(crate) fn for_daemon(caller: &CallerPath) -> Self {
        let callers_dirs = caller.absolute_dirs();
        let tool_dirs: Vec<&PathBuf> = DAEMON_TOOLS
            .iter()
            .filter_map(|tool| callers_dirs.iter().find(|dir| dir.join(tool).is_file()))
            .collect();
        let dirs: Vec<&str> = callers_dirs
            .iter()
            .filter(|dir| tool_dirs.contains(dir))
            .filter_map(|dir| dir.to_str())
            .filter(|dir| !SYSTEM_DIRS.contains(dir))
            .chain(SYSTEM_DIRS)
            .collect();
        Self(dirs.join(":"))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// The `hub-daemon` binary `just install` put on the caller's `PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstalledDaemon(pub(super) AbsolutePath);

impl InstalledDaemon {
    /// The first `hub-daemon` in an absolute directory of `caller`, the one the caller's
    /// shell would run.
    ///
    /// # Errors
    /// Returns an error saying to run `just install` when there is none.
    pub(crate) fn find(caller: &CallerPath) -> Result<Self> {
        let installed = caller
            .absolute_dirs()
            .into_iter()
            .map(|dir| dir.join("hub-daemon"))
            .find(|candidate| candidate.is_file())
            .context("hub-daemon is not on PATH: run `just install` first")?;
        Ok(Self(AbsolutePath::new(&installed)?))
    }

    pub(crate) const fn path(&self) -> &AbsolutePath {
        &self.0
    }
}

/// The main checkout, never a worktree, because a worktree is deleted when its branch is done
/// and a daemon started in a deleted directory fails at every start. It holds `hub.toml`, which
/// the daemon reads from its working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MainCheckout(pub(super) AbsolutePath);

impl MainCheckout {
    /// The checkout that owns `common_dir`, git's shared directory, which is the same from the
    /// main checkout and from every worktree.
    ///
    /// # Errors
    /// Returns an error when that checkout has no `hub.toml`.
    pub(crate) fn of(common_dir: &AbsolutePath) -> Result<Self> {
        let git_dir = common_dir.as_path();
        let checkout = git_dir.parent().unwrap_or(git_dir);
        if !checkout.join("hub.toml").is_file() {
            bail!(
                "{} has no hub.toml, which the daemon reads at startup: \
                 run `just setup-private <device>` or copy hub.toml.example",
                checkout.display()
            );
        }
        Ok(Self(AbsolutePath::new(checkout)?))
    }

    pub(crate) const fn path(&self) -> &AbsolutePath {
        &self.0
    }
}

/// Where launchd sends the daemon's stdout and stderr: every startup line, startup error and
/// pass line, in the `default` profile's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogFile(pub(super) AbsolutePath);

impl LogFile {
    /// # Errors
    /// Returns an error when the joined path is not UTF-8, which only a non-UTF-8 `home` can make.
    pub(crate) fn of_default_profile(home: &AbsolutePath) -> Result<Self> {
        let path = Profile::Default.dir(home.as_path()).join("daemon.log");
        Ok(Self(AbsolutePath::new(&path)?))
    }

    pub(crate) const fn path(&self) -> &AbsolutePath {
        &self.0
    }
}

/// Everything the agent definition is rendered from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchAgent {
    pub(crate) program: InstalledDaemon,
    pub(crate) checkout: MainCheckout,
    pub(crate) search_path: SearchPath,
    pub(crate) log: LogFile,
}

/// The directory a `PATH` entry names, when launchd could use it as written.
fn absolute_dir(entry: &str) -> Option<PathBuf> {
    let dir = Path::new(entry);
    dir.is_absolute().then(|| dir.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn absolute(path: &str) -> AbsolutePath {
        AbsolutePath::new(Path::new(path)).unwrap()
    }

    fn executable(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn a_relative_path_is_refused_naming_it() {
        let error = AbsolutePath::new(Path::new("relative/home")).unwrap_err();

        assert!(error.to_string().contains("relative/home"), "{error}");
    }

    #[test]
    fn an_absolute_path_is_kept_as_written() {
        assert_eq!(absolute("/Users/someone").as_str(), "/Users/someone");
    }

    #[test]
    fn an_unset_or_empty_path_is_refused() {
        assert!(CallerPath::new(None).is_err());
        assert!(CallerPath::new(Some(String::new())).is_err());
    }

    fn caller(dirs: &[&Path]) -> CallerPath {
        let joined: Vec<String> = dirs.iter().map(|dir| dir.display().to_string()).collect();
        CallerPath::new(Some(joined.join(":"))).unwrap()
    }

    #[test]
    fn the_daemons_path_is_where_its_tools_are_then_the_system_directories() {
        let dir = tempfile::tempdir().unwrap();
        let homebrew = dir.path().join("homebrew/bin");
        let sdk = dir.path().join("sdk/bin");
        executable(&homebrew.join("op"));
        executable(&homebrew.join("git"));
        executable(&sdk.join("gcloud"));

        let search_path = SearchPath::for_daemon(&caller(&[&homebrew, &sdk]));

        assert_eq!(
            search_path.as_str(),
            format!(
                "{}:{}:/usr/bin:/bin:/usr/sbin:/sbin",
                homebrew.display(),
                sdk.display()
            )
        );
    }

    #[test]
    fn a_per_session_directory_holding_none_of_the_tools_is_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("fnm_multishells/13320_1791254324586/bin");
        let homebrew = dir.path().join("homebrew/bin");
        executable(&session.join("node"));
        executable(&homebrew.join("op"));

        let search_path = SearchPath::for_daemon(&caller(&[&session, &homebrew]));

        assert!(
            !search_path.as_str().contains("fnm_multishells"),
            "{search_path:?}"
        );
    }

    #[test]
    fn a_tool_on_the_path_twice_contributes_only_the_directory_the_shell_would_use() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        executable(&first.join("git"));
        executable(&second.join("git"));

        let search_path = SearchPath::for_daemon(&caller(&[&first, &second]));

        assert_eq!(
            search_path.as_str(),
            format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", first.display())
        );
    }

    #[test]
    fn without_any_tool_on_the_path_the_daemon_gets_the_system_directories() {
        let dir = tempfile::tempdir().unwrap();

        let search_path = SearchPath::for_daemon(&caller(&[dir.path()]));

        assert_eq!(search_path.as_str(), "/usr/bin:/bin:/usr/sbin:/sbin");
    }

    #[test]
    fn a_tool_directory_already_among_the_system_directories_is_not_repeated() {
        let search_path = SearchPath::for_daemon(&caller(&[Path::new("/usr/bin")]));

        assert_eq!(
            search_path.as_str().matches("/usr/bin").count(),
            1,
            "{search_path:?}"
        );
    }

    #[test]
    fn the_first_hub_daemon_on_the_path_is_the_installed_one() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first/hub-daemon");
        let second = dir.path().join("second/hub-daemon");
        executable(&first);
        executable(&second);
        let found = InstalledDaemon::find(&caller(&[
            first.parent().unwrap(),
            second.parent().unwrap(),
        ]))
        .unwrap();

        assert_eq!(found.path().as_path(), first);
    }

    #[test]
    fn a_relative_path_entry_is_skipped_because_launchd_would_resolve_it_from_root() {
        let dir = tempfile::tempdir().unwrap();
        let installed = dir.path().join("bin/hub-daemon");
        executable(&installed);
        let found =
            InstalledDaemon::find(&caller(&[Path::new("."), installed.parent().unwrap()])).unwrap();

        assert_eq!(found.path().as_path(), installed);
    }

    #[test]
    fn without_hub_daemon_on_the_path_the_refusal_says_to_install_it() {
        let dir = tempfile::tempdir().unwrap();
        let error = InstalledDaemon::find(&caller(&[dir.path()])).unwrap_err();

        assert!(error.to_string().contains("just install"), "{error}");
    }

    #[test]
    fn a_directory_named_hub_daemon_is_not_the_binary() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("hub-daemon")).unwrap();
        assert!(InstalledDaemon::find(&caller(&[dir.path()])).is_err());
    }

    #[test]
    fn the_main_checkout_is_the_directory_holding_the_shared_git_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("hub/.git")).unwrap();
        std::fs::write(dir.path().join("hub/hub.toml"), "").unwrap();
        let common_dir = AbsolutePath::new(&dir.path().join("hub/.git")).unwrap();

        let checkout = MainCheckout::of(&common_dir).unwrap();

        assert_eq!(checkout.path().as_path(), dir.path().join("hub"));
    }

    #[test]
    fn a_checkout_without_hub_toml_is_refused_naming_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("hub/.git")).unwrap();
        let common_dir = AbsolutePath::new(&dir.path().join("hub/.git")).unwrap();

        let error = MainCheckout::of(&common_dir).unwrap_err();

        assert!(error.to_string().contains("hub.toml"), "{error}");
    }

    #[test]
    fn the_log_is_in_the_default_profile_directory() {
        let log = LogFile::of_default_profile(&absolute("/Users/someone")).unwrap();

        assert_eq!(
            log.path().as_str(),
            "/Users/someone/.hub/default/daemon.log"
        );
    }
}
