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
}

/// The steps that bring the job from `situation` to what its command asks for.
pub(crate) fn plan(situation: Situation) -> Vec<AgentStep> {
    use AgentStep::{Bootout, Bootstrap, WriteDefinition};
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
        }
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
        })
    }
}

/// Halts on a step list that only a bug in `plan` could produce: one that bootstraps a loaded
/// job without unloading it first, which launchd refuses.
fn assert_steps_fit(job: JobState, steps: &[AgentStep]) {
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

    use super::AgentStep::{Bootout, Bootstrap, WriteDefinition};
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
    fn the_outcome_says_what_changed(#[case] situation: Situation, #[case] expected: AgentOutcome) {
        assert_eq!(AgentOutcome::of(situation, &plan(situation)), expected);
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
}
