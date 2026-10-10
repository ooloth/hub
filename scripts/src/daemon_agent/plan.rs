//! What each command does to the job, given what launchd and the disk hold. Pure.
use std::fmt;

use super::definition::DefinitionOnDisk;

/// Whether launchd has the job loaded in the user's GUI domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobState {
    Loaded,
    NotLoaded,
}

/// A command, with the state it depends on and nothing else. Stopping does not depend on the
/// definition, so a stop works however the definition or the installed binary has changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Situation {
    Starting {
        job: JobState,
        on_disk: DefinitionOnDisk,
    },
    Stopping {
        job: JobState,
    },
    Restarting {
        job: JobState,
    },
    Uninstalling {
        job: JobState,
    },
    /// What `just install` does after installing a new binary. The definition is compared only
    /// when the job is loaded, so `None` means launchd does not have the job and nothing about
    /// the definition was needed.
    RestartingIfRunning {
        running: Option<DefinitionOnDisk>,
    },
}

/// One change to the job, run in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentStep {
    /// Write the rendered definition over the installed one.
    WriteDefinition,
    /// Unload the job, stopping the daemon. The definition stays, so launchd loads it again at
    /// the next login.
    Bootout,
    /// Load the job from the installed definition, starting the daemon.
    Bootstrap,
    /// Kill the running daemon and start it again from the loaded definition
    /// (`launchctl kickstart -k`), which picks up a binary replaced at the same path.
    Kickstart,
    /// Delete the installed definition, so launchd does not load it at the next login.
    RemoveDefinition,
}

/// The steps that bring the job from `situation` to what its command asks for.
pub(crate) fn plan(situation: Situation) -> Vec<AgentStep> {
    use AgentStep::{Bootout, Bootstrap, Kickstart, RemoveDefinition, WriteDefinition};
    use DefinitionOnDisk::{Different, Missing, Same};
    use JobState::{Loaded, NotLoaded};

    let (job, steps) = match situation {
        Situation::Starting { job, on_disk } => (
            job,
            match (job, on_disk) {
                (NotLoaded, Missing | Different) => vec![WriteDefinition, Bootstrap],
                (NotLoaded, Same) => vec![Bootstrap],
                (Loaded, Missing | Different) => vec![WriteDefinition, Bootout, Bootstrap],
                (Loaded, Same) => vec![],
            },
        ),
        Situation::Stopping { job } => (
            job,
            match job {
                Loaded => vec![Bootout],
                NotLoaded => vec![],
            },
        ),
        Situation::Restarting { job } => (
            job,
            match job {
                Loaded => vec![Kickstart],
                NotLoaded => vec![],
            },
        ),
        Situation::Uninstalling { job } => (
            job,
            match job {
                Loaded => vec![Bootout, RemoveDefinition],
                NotLoaded => vec![RemoveDefinition],
            },
        ),
        Situation::RestartingIfRunning { running: None } => (NotLoaded, vec![]),
        Situation::RestartingIfRunning {
            running: Some(on_disk),
        } => (
            Loaded,
            match on_disk {
                Same => vec![Kickstart],
                Missing | Different => vec![WriteDefinition, Bootout, Bootstrap],
            },
        ),
    };
    assert_steps_fit(job, &steps);
    steps
}

/// What a command changed, from what it found and the steps it ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentOutcome {
    Started,
    /// The definition changed, so the job was reloaded from the new one.
    Reloaded,
    AlreadyRunning,
    Stopped,
    AlreadyStopped,
    Restarted,
    /// A restart was asked of a job launchd does not have. The only outcome that is a failure.
    NotRunning,
    Uninstalled,
    /// `just install` found the job not loaded and left it so.
    LeftStopped,
}

impl AgentOutcome {
    pub(crate) fn of(situation: Situation, steps: &[AgentStep]) -> Self {
        let bootstraps = steps.contains(&AgentStep::Bootstrap);
        let boots_out = steps.contains(&AgentStep::Bootout);
        match situation {
            Situation::Starting { .. } if bootstraps && boots_out => Self::Reloaded,
            Situation::Starting { .. } if bootstraps => Self::Started,
            Situation::Starting { .. } => Self::AlreadyRunning,
            Situation::Stopping { .. } if boots_out => Self::Stopped,
            Situation::Stopping { .. } => Self::AlreadyStopped,
            Situation::Restarting { .. } if steps.contains(&AgentStep::Kickstart) => {
                Self::Restarted
            }
            Situation::Restarting { .. } => Self::NotRunning,
            Situation::Uninstalling { .. } => Self::Uninstalled,
            Situation::RestartingIfRunning { running: None } => Self::LeftStopped,
            Situation::RestartingIfRunning { .. } if bootstraps => Self::Reloaded,
            Situation::RestartingIfRunning { .. } => Self::Restarted,
        }
    }

    /// Whether the command failed to do what it was asked, which exits 1.
    pub(crate) const fn is_failure(self) -> bool {
        matches!(self, Self::NotRunning)
    }
}

impl fmt::Display for AgentOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Started => "started hub-daemon; it starts again at every login",
            Self::Reloaded => "wrote the new agent definition and restarted hub-daemon",
            Self::AlreadyRunning => {
                "hub-daemon is already running with this definition; nothing changed"
            }
            Self::Stopped => {
                "stopped hub-daemon until the next login, or until `just daemon-start`"
            }
            Self::AlreadyStopped => "hub-daemon is not running; nothing to stop",
            Self::Restarted => "restarted hub-daemon",
            Self::NotRunning => "hub-daemon is not running; start it with `just daemon-start`",
            Self::Uninstalled => {
                "removed hub-daemon's LaunchAgent; it no longer starts at login, until `just daemon-start`"
            }
            Self::LeftStopped => {
                "hub-daemon is not running under launchd; if it is installed, the new binary starts at the next login"
            }
        })
    }
}

/// Halts on a step list that only a bug in `plan` could produce: one that bootstraps a loaded
/// job without unloading it first, which launchd refuses; one that kickstarts a job launchd does
/// not have; or one that both writes and removes the definition.
fn assert_steps_fit(job: JobState, steps: &[AgentStep]) {
    assert!(
        job == JobState::Loaded || !steps.contains(&AgentStep::Kickstart),
        "plan kickstarts a job launchd does not have: {steps:?}"
    );
    assert!(
        !(steps.contains(&AgentStep::WriteDefinition)
            && steps.contains(&AgentStep::RemoveDefinition)),
        "plan both writes and removes the definition: {steps:?}"
    );
    let first_bootstrap = steps.iter().position(|step| *step == AgentStep::Bootstrap);
    let first_bootout = steps.iter().position(|step| *step == AgentStep::Bootout);
    if let (JobState::Loaded, Some(bootstrap)) = (job, first_bootstrap) {
        assert!(
            first_bootout.is_some_and(|bootout| bootout < bootstrap),
            "plan bootstraps a loaded job without unloading it first: {steps:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::AgentStep::{Bootout, Bootstrap, Kickstart, RemoveDefinition, WriteDefinition};
    use super::DefinitionOnDisk::{Different, Missing, Same};
    use super::JobState::{Loaded, NotLoaded};
    use super::*;

    #[rstest]
    #[case::first_start(NotLoaded, Missing, &[WriteDefinition, Bootstrap])]
    #[case::stopped_with_a_changed_definition(NotLoaded, Different, &[WriteDefinition, Bootstrap])]
    #[case::stopped_with_the_same_definition(NotLoaded, Same, &[Bootstrap])]
    #[case::running_with_its_definition_removed(Loaded, Missing, &[WriteDefinition, Bootout, Bootstrap])]
    #[case::running_with_a_changed_definition(Loaded, Different, &[WriteDefinition, Bootout, Bootstrap])]
    #[case::running_with_the_same_definition(Loaded, Same, &[])]
    fn starting(
        #[case] job: JobState,
        #[case] on_disk: DefinitionOnDisk,
        #[case] expected: &[AgentStep],
    ) {
        assert_eq!(plan(Situation::Starting { job, on_disk }), expected);
    }

    #[rstest]
    #[case::running(Loaded, &[Bootout])]
    #[case::not_running(NotLoaded, &[])]
    fn stopping(#[case] job: JobState, #[case] expected: &[AgentStep]) {
        assert_eq!(plan(Situation::Stopping { job }), expected);
    }

    #[rstest]
    #[case::running(Loaded, &[Kickstart])]
    #[case::not_running(NotLoaded, &[])]
    fn restarting(#[case] job: JobState, #[case] expected: &[AgentStep]) {
        assert_eq!(plan(Situation::Restarting { job }), expected);
    }

    #[rstest]
    #[case::running(Loaded, &[Bootout, RemoveDefinition])]
    #[case::not_running(NotLoaded, &[RemoveDefinition])]
    fn uninstalling(#[case] job: JobState, #[case] expected: &[AgentStep]) {
        assert_eq!(plan(Situation::Uninstalling { job }), expected);
    }

    #[rstest]
    #[case::not_running(None, &[])]
    #[case::running_with_the_same_definition(Some(Same), &[Kickstart])]
    #[case::running_with_a_changed_definition(Some(Different), &[WriteDefinition, Bootout, Bootstrap])]
    #[case::running_with_its_definition_removed(Some(Missing), &[WriteDefinition, Bootout, Bootstrap])]
    fn restarting_if_running(
        #[case] running: Option<DefinitionOnDisk>,
        #[case] expected: &[AgentStep],
    ) {
        assert_eq!(plan(Situation::RestartingIfRunning { running }), expected);
    }

    #[test]
    fn an_unchanged_definition_is_never_rewritten() {
        for job in [Loaded, NotLoaded] {
            let steps = plan(Situation::Starting { job, on_disk: Same });

            assert!(!steps.contains(&WriteDefinition), "{job:?}: {steps:?}");
        }
    }

    #[rstest]
    #[case::started(Situation::Starting { job: NotLoaded, on_disk: Missing }, AgentOutcome::Started)]
    #[case::reloaded(Situation::Starting { job: Loaded, on_disk: Different }, AgentOutcome::Reloaded)]
    #[case::already_running(Situation::Starting { job: Loaded, on_disk: Same }, AgentOutcome::AlreadyRunning)]
    #[case::stopped(Situation::Stopping { job: Loaded }, AgentOutcome::Stopped)]
    #[case::already_stopped(Situation::Stopping { job: NotLoaded }, AgentOutcome::AlreadyStopped)]
    #[case::restarted(Situation::Restarting { job: Loaded }, AgentOutcome::Restarted)]
    #[case::not_running(Situation::Restarting { job: NotLoaded }, AgentOutcome::NotRunning)]
    #[case::uninstalled_while_running(Situation::Uninstalling { job: Loaded }, AgentOutcome::Uninstalled)]
    #[case::uninstalled_while_stopped(Situation::Uninstalling { job: NotLoaded }, AgentOutcome::Uninstalled)]
    #[case::install_left_it_stopped(Situation::RestartingIfRunning { running: None }, AgentOutcome::LeftStopped)]
    #[case::install_restarted_it(Situation::RestartingIfRunning { running: Some(Same) }, AgentOutcome::Restarted)]
    #[case::install_reloaded_it(Situation::RestartingIfRunning { running: Some(Different) }, AgentOutcome::Reloaded)]
    fn the_outcome_says_what_changed(#[case] situation: Situation, #[case] expected: AgentOutcome) {
        assert_eq!(AgentOutcome::of(situation, &plan(situation)), expected);
    }

    #[test]
    fn only_restarting_a_daemon_that_is_not_running_is_a_failure() {
        let failures: Vec<AgentOutcome> = [
            AgentOutcome::Started,
            AgentOutcome::Reloaded,
            AgentOutcome::AlreadyRunning,
            AgentOutcome::Stopped,
            AgentOutcome::AlreadyStopped,
            AgentOutcome::Restarted,
            AgentOutcome::NotRunning,
            AgentOutcome::Uninstalled,
            AgentOutcome::LeftStopped,
        ]
        .into_iter()
        .filter(|outcome| outcome.is_failure())
        .collect();

        assert_eq!(failures, [AgentOutcome::NotRunning]);
    }

    #[test]
    fn a_failed_restart_says_how_to_start_the_daemon() {
        assert!(AgentOutcome::NotRunning
            .to_string()
            .contains("just daemon-start"));
    }

    #[test]
    fn leaving_the_daemon_stopped_says_when_the_new_binary_runs() {
        assert!(AgentOutcome::LeftStopped.to_string().contains("next login"));
    }

    #[test]
    fn a_stop_says_when_the_daemon_comes_back() {
        assert!(AgentOutcome::Stopped.to_string().contains("next login"));
    }

    #[test]
    #[should_panic(expected = "bootstraps a loaded job")]
    fn bootstrapping_a_loaded_job_without_unloading_it_halts() {
        assert_steps_fit(Loaded, &[WriteDefinition, Bootstrap]);
    }

    #[test]
    fn bootstrapping_a_loaded_job_after_unloading_it_fits() {
        assert_steps_fit(Loaded, &[WriteDefinition, Bootout, Bootstrap]);
    }

    #[test]
    #[should_panic(expected = "kickstarts a job launchd does not have")]
    fn kickstarting_a_job_that_is_not_loaded_halts() {
        assert_steps_fit(NotLoaded, &[Kickstart]);
    }

    #[test]
    #[should_panic(expected = "both writes and removes the definition")]
    fn writing_and_removing_the_definition_in_one_plan_halts() {
        assert_steps_fit(Loaded, &[WriteDefinition, Bootout, RemoveDefinition]);
    }
}
