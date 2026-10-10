//! The `LaunchAgent` plist for `hub-daemon`, and how it compares to the one already installed.
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::launch_agent::{AbsolutePath, LaunchAgent};

/// The job's name in launchd, and the plist's file name.
pub(crate) const LABEL: &str = "com.ooloth.hub.daemon";

/// The least time between two launches. A daemon that cannot start, such as one whose
/// 1Password app is not running yet at login, fails within a second, and launchd restarts it at
/// most this often. 60 s writes about 0.6 MB of failure lines a day; launchd's default of 10 s
/// writes about 3 MB.
const THROTTLE_INTERVAL_SECS: u32 = 60;

/// The plist's keys, in the order they are written. Its environment holds `PATH` and nothing
/// else, so neither `HUB_PROFILE` nor a credential can be written into it: an unset
/// `HUB_PROFILE` is what makes the daemon use the `default` profile (Decision 024).
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct LaunchdJob {
    label: String,
    program_arguments: Vec<String>,
    working_directory: String,
    environment_variables: JobEnvironment,
    standard_out_path: String,
    standard_error_path: String,
    run_at_load: bool,
    keep_alive: bool,
    throttle_interval: u32,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobEnvironment {
    #[serde(rename = "PATH")]
    path: String,
}

/// The plist as written to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentDefinition(Vec<u8>);

impl AgentDefinition {
    /// # Errors
    /// Returns an error when the plist cannot be serialized.
    pub(crate) fn render(agent: &LaunchAgent) -> Result<Self> {
        let log = agent.log.path().as_str().to_string();
        let job = LaunchdJob {
            label: LABEL.to_string(),
            program_arguments: vec![agent.program.path().as_str().to_string()],
            working_directory: agent.checkout.path().as_str().to_string(),
            environment_variables: JobEnvironment {
                path: agent.search_path.as_str().to_string(),
            },
            standard_out_path: log.clone(),
            standard_error_path: log,
            run_at_load: true,
            keep_alive: true,
            throttle_interval: THROTTLE_INTERVAL_SECS,
        };
        let mut bytes = Vec::new();
        plist::to_writer_xml(&mut bytes, &job).context("failed to render the agent definition")?;
        Ok(Self(bytes))
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }
}

/// How the installed plist compares to the one just rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefinitionOnDisk {
    Missing,
    Same,
    Different,
}

impl DefinitionOnDisk {
    /// Byte for byte: rewriting a plist, even with identical content, makes macOS post a
    /// Background Items notification, so only a difference is worth a write.
    pub(crate) fn compare(installed: Option<&[u8]>, rendered: &AgentDefinition) -> Self {
        match installed {
            None => Self::Missing,
            Some(bytes) if bytes == rendered.bytes() => Self::Same,
            Some(_) => Self::Different,
        }
    }
}

/// Where launchd loads the plist from at every login.
pub(crate) fn definition_path(home: &AbsolutePath) -> PathBuf {
    home.as_path()
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::super::launch_agent::{InstalledDaemon, LogFile, MainCheckout, SearchPath};
    use super::*;

    fn absolute(path: &str) -> AbsolutePath {
        AbsolutePath::new(std::path::Path::new(path)).unwrap()
    }

    fn agent(program: &str, checkout: &str, search_path: &str, log: &str) -> LaunchAgent {
        LaunchAgent {
            program: InstalledDaemon(absolute(program)),
            checkout: MainCheckout(absolute(checkout)),
            search_path: SearchPath(search_path.to_string()),
            log: LogFile(absolute(log)),
        }
    }

    fn fixed_agent() -> LaunchAgent {
        agent(
            "/Users/someone/.cargo/bin/hub-daemon",
            "/Users/someone/code/hub",
            "/opt/homebrew/bin:/usr/bin:/bin",
            "/Users/someone/.hub/default/daemon.log",
        )
    }

    fn parse(definition: &AgentDefinition) -> LaunchdJob {
        plist::from_bytes(definition.bytes()).unwrap()
    }

    #[test]
    fn a_rendered_definition_matches_the_reviewed_plist() {
        let rendered = AgentDefinition::render(&fixed_agent()).unwrap();

        insta::assert_snapshot!(String::from_utf8(rendered.bytes().to_vec()).unwrap());
    }

    #[test]
    fn the_job_environment_holds_path_and_nothing_else() {
        let rendered = AgentDefinition::render(&fixed_agent()).unwrap();
        let value: plist::Value = plist::from_bytes(rendered.bytes()).unwrap();

        let keys: Vec<&String> = value
            .as_dictionary()
            .and_then(|job| job.get("EnvironmentVariables"))
            .and_then(plist::Value::as_dictionary)
            .map(|environment| environment.keys().collect())
            .unwrap_or_default();

        assert_eq!(keys, ["PATH"]);
    }

    #[test]
    fn stdout_and_stderr_go_to_the_same_log() {
        let job = parse(&AgentDefinition::render(&fixed_agent()).unwrap());

        assert_eq!(job.standard_out_path, job.standard_error_path);
    }

    proptest! {
        #[test]
        fn any_paths_render_to_a_plist_that_reads_back_the_same(
            program in "/[^\\x00-\\x1f]{0,40}",
            checkout in "/[^\\x00-\\x1f]{0,40}",
            search_path in "[^\\x00-\\x1f]{1,40}",
            log in "/[^\\x00-\\x1f]{0,40}",
        ) {
            let job = parse(
                &AgentDefinition::render(&agent(&program, &checkout, &search_path, &log)).unwrap(),
            );

            prop_assert_eq!(job.program_arguments, vec![program]);
            prop_assert_eq!(job.working_directory, checkout);
            prop_assert_eq!(job.environment_variables.path, search_path);
            prop_assert_eq!(job.standard_out_path, log);
        }
    }

    #[test]
    fn paths_with_markup_characters_and_spaces_read_back_the_same() {
        let job = parse(
            &AgentDefinition::render(&agent(
                "/Users/a & b/<bin>/hub-daemon",
                "/Users/a & b/code/\"hub\"",
                "/opt/x y:/usr/bin",
                "/Users/a & b/.hub/default/daemon.log",
            ))
            .unwrap(),
        );

        assert_eq!(job.program_arguments, ["/Users/a & b/<bin>/hub-daemon"]);
        assert_eq!(job.working_directory, "/Users/a & b/code/\"hub\"");
    }

    #[test]
    fn no_installed_definition_is_missing() {
        let rendered = AgentDefinition::render(&fixed_agent()).unwrap();

        assert_eq!(
            DefinitionOnDisk::compare(None, &rendered),
            DefinitionOnDisk::Missing
        );
    }

    #[test]
    fn identical_bytes_are_the_same() {
        let rendered = AgentDefinition::render(&fixed_agent()).unwrap();

        assert_eq!(
            DefinitionOnDisk::compare(Some(rendered.bytes()), &rendered),
            DefinitionOnDisk::Same
        );
    }

    #[test]
    fn any_other_bytes_are_different() {
        let rendered = AgentDefinition::render(&fixed_agent()).unwrap();
        let other = AgentDefinition::render(&agent(
            "/elsewhere/hub-daemon",
            "/Users/someone/code/hub",
            "/usr/bin",
            "/Users/someone/.hub/default/daemon.log",
        ))
        .unwrap();

        assert_eq!(
            DefinitionOnDisk::compare(Some(other.bytes()), &rendered),
            DefinitionOnDisk::Different
        );
    }

    #[test]
    fn the_definition_lives_in_the_users_launch_agents_named_by_its_label() {
        assert_eq!(
            definition_path(&absolute("/Users/someone")),
            PathBuf::from("/Users/someone/Library/LaunchAgents/com.ooloth.hub.daemon.plist")
        );
    }
}
