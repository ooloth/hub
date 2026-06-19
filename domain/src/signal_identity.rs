use crate::pr::RepoSlug;
use crate::task_origin::{AlertSource, IssueSystem, TaskOrigin};

/// The stable identity of a signal across multiple fetches.
///
/// Differs from [`TaskOrigin`] in that volatile display-only fields
/// (`Ci::url`, `Alert::label`) are excluded, so two instances can be
/// compared by [`Eq`] without spurious mismatches caused by per-run or
/// per-scan values changing.
///
/// Used as the lookup key for task↔signal matching in `build_unified` and as
/// the comparison target in `store::tasks::active_for_origin`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum SignalIdentity {
    /// A pull request identified by repository and number.
    Pr {
        /// Repository the pull request belongs to.
        repo: RepoSlug,
        /// Pull request number within the repository.
        number: u64,
    },
    /// A ticket in a supported issue tracker.
    Issue {
        /// Issue tracker the ticket comes from.
        system: IssueSystem,
        /// Repository scope for GitHub issues; `None` for Linear.
        repo: Option<RepoSlug>,
        /// Tracker-unique issue identifier.
        id: String,
    },
    /// Identity = `(repo, workflow, job, step)`. `url` is excluded — it changes
    /// on every CI run and must never influence whether a task matches.
    Ci {
        /// Repository the workflow belongs to.
        repo: RepoSlug,
        /// Workflow name (e.g. "CI").
        workflow: String,
        /// Job name within the workflow, if applicable.
        job: Option<String>,
        /// Step name within the job, if applicable.
        step: Option<String>,
    },
    /// Identity = `(source, key)`. `label` is excluded — it is a display string
    /// that may change between scans without the underlying problem changing.
    Alert {
        /// Scan source that produced this alert.
        source: AlertSource,
        /// Stable correlation key (e.g. `project/env/message`).
        key: String,
    },
    /// A blank task with no originating signal.
    Idea,
}

impl From<&TaskOrigin> for SignalIdentity {
    fn from(origin: &TaskOrigin) -> Self {
        match origin {
            TaskOrigin::Pr { repo, number } => Self::Pr {
                repo: repo.clone(),
                number: *number,
            },
            TaskOrigin::Issue { system, repo, id } => Self::Issue {
                system: *system,
                repo: repo.clone(),
                id: id.clone(),
            },
            TaskOrigin::Ci {
                repo,
                workflow,
                job,
                step,
                ..
            } => Self::Ci {
                repo: repo.clone(),
                workflow: workflow.clone(),
                job: job.clone(),
                step: step.clone(),
            },
            TaskOrigin::Alert { source, key, .. } => Self::Alert {
                source: *source,
                key: key.clone(),
            },
            TaskOrigin::Idea => Self::Idea,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn slug() -> RepoSlug {
        RepoSlug::new("ooloth", "hub")
    }

    // ── volatile fields are stripped ─────────────────────────────────────────

    #[test]
    fn ci_origins_differing_only_in_url_are_equal() {
        let a = TaskOrigin::Ci {
            repo: slug(),
            workflow: "ci".into(),
            job: Some("test".into()),
            step: None,
            url: "https://github.com/actions/runs/1".into(),
        };
        let b = TaskOrigin::Ci {
            repo: slug(),
            workflow: "ci".into(),
            job: Some("test".into()),
            step: None,
            url: "https://github.com/actions/runs/9999".into(),
        };
        assert_eq!(SignalIdentity::from(&a), SignalIdentity::from(&b));
    }

    #[test]
    fn alert_origins_differing_only_in_label_are_equal() {
        let a = TaskOrigin::Alert {
            source: AlertSource::Loki,
            key: "app/prod/db-timeout".into(),
            label: "old label".into(),
        };
        let b = TaskOrigin::Alert {
            source: AlertSource::Loki,
            key: "app/prod/db-timeout".into(),
            label: "new label after rescan".into(),
        };
        assert_eq!(SignalIdentity::from(&a), SignalIdentity::from(&b));
    }

    // ── identity fields are preserved ────────────────────────────────────────

    #[test]
    fn pr_origins_with_different_numbers_are_unequal() {
        let a = TaskOrigin::Pr {
            repo: slug(),
            number: 1,
        };
        let b = TaskOrigin::Pr {
            repo: slug(),
            number: 2,
        };
        assert_ne!(SignalIdentity::from(&a), SignalIdentity::from(&b));
    }

    #[test]
    fn ci_origins_differing_in_workflow_are_unequal() {
        let a = TaskOrigin::Ci {
            repo: slug(),
            workflow: "ci".into(),
            job: None,
            step: None,
            url: "u".into(),
        };
        let b = TaskOrigin::Ci {
            repo: slug(),
            workflow: "deploy".into(),
            job: None,
            step: None,
            url: "u".into(),
        };
        assert_ne!(SignalIdentity::from(&a), SignalIdentity::from(&b));
    }

    #[test]
    fn alert_origins_differing_in_key_are_unequal() {
        let a = TaskOrigin::Alert {
            source: AlertSource::Loki,
            key: "app/prod/db-timeout".into(),
            label: "same".into(),
        };
        let b = TaskOrigin::Alert {
            source: AlertSource::Loki,
            key: "app/prod/oom-kill".into(),
            label: "same".into(),
        };
        assert_ne!(SignalIdentity::from(&a), SignalIdentity::from(&b));
    }

    // ── different variants are never equal ───────────────────────────────────

    #[rstest]
    #[case(
        TaskOrigin::Pr { repo: slug(), number: 1 },
        TaskOrigin::Issue { system: IssueSystem::GitHub, repo: Some(slug()), id: "1".into() }
    )]
    #[case(
        TaskOrigin::Ci { repo: slug(), workflow: "ci".into(), job: None, step: None, url: "u".into() },
        TaskOrigin::Alert { source: AlertSource::Loki, key: "k".into(), label: "l".into() }
    )]
    #[case(
        TaskOrigin::Idea,
        TaskOrigin::Pr { repo: slug(), number: 1 }
    )]
    fn different_variants_are_unequal(#[case] a: TaskOrigin, #[case] b: TaskOrigin) {
        assert_ne!(SignalIdentity::from(&a), SignalIdentity::from(&b));
    }
}
