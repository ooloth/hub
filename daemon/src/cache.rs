//! Recording a pass in hub's database: its health record always, and the payload too when the
//! pass replaced it.

use anyhow::{Context, Result};
use domain::daemon_pass::{Pass, PassOutcome};
use domain::known_secrets::KnownSecrets;
use domain::pass_failure::PassFailure;
use domain::profile::Profile;
use rusqlite::Connection;

use crate::freshness::FreshPayload;

/// Whether a pass reached the database, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Recording {
    /// The pass was recorded as it ran.
    Recorded,
    /// Recording the pass failed, so it was recorded as a failed pass instead, health only.
    RecordedAsFailed {
        /// Why recording the pass as it ran failed.
        error: PassFailure,
    },
    /// Nothing reached the database, so the health record's timestamp stops advancing.
    Unrecorded {
        /// Why recording the pass as it ran failed.
        error: PassFailure,
        /// Why recording it as failed failed too, when that was tried.
        fallback_error: Option<PassFailure>,
    },
}

/// Opens `profile`'s database with both of the tables a pass writes.
///
/// # Errors
/// Returns an error if the database cannot be opened or a table cannot be prepared.
pub(crate) fn open(profile: Profile) -> Result<Connection> {
    let conn = store::status_cache::connect(profile).context("failed to open the hub database")?;
    prepare(&conn)?;
    Ok(conn)
}

fn prepare(conn: &Connection) -> Result<()> {
    store::status_cache::ensure_table(conn).context("failed to prepare the status cache")?;
    store::daemon_health::ensure_table(conn).context("failed to prepare the daemon health record")
}

/// Records `pass`, with `payload` when it replaced the payload, in one transaction.
///
/// When that fails, records the pass once more as failed, health only, so a database that refuses
/// only the payload still says why. Every error is redacted of `secrets` before it is kept.
pub(crate) fn record(
    open: impl Fn() -> Result<Connection>,
    pass: &Pass,
    payload: Option<&FreshPayload>,
    secrets: &KnownSecrets,
) -> Recording {
    let Err(error) = write(&open, pass, payload.map(FreshPayload::payload)) else {
        return Recording::Recorded;
    };
    let error = PassFailure::new(&error, secrets);
    // A pass that had already failed was written health only, so writing that shape again would
    // fail the same way.
    if matches!(pass.outcome, PassOutcome::Failed { .. }) {
        return Recording::Unrecorded {
            error,
            fallback_error: None,
        };
    }
    match write(&open, &pass.failed_with(error.clone()), None) {
        Ok(()) => Recording::RecordedAsFailed { error },
        Err(fallback_error) => Recording::Unrecorded {
            error,
            fallback_error: Some(PassFailure::new(&fallback_error, secrets)),
        },
    }
}

fn write(
    open: &impl Fn() -> Result<Connection>,
    pass: &Pass,
    payload: Option<&store::status_cache::Payload>,
) -> Result<()> {
    let conn = open()?;
    store::daemon_health::record(&conn, pass, payload)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use chrono::DateTime;
    use domain::source_failure::SourceFailure;
    use domain::source_outcomes::SourceOutcomes;
    use rusqlite::OpenFlags;
    use secrecy::Secret;

    use super::*;
    use crate::freshness::{classify, RefreshOutcome};

    fn no_secrets() -> KnownSecrets {
        KnownSecrets::new(Vec::new())
    }

    /// A file-backed database in a fresh directory, so a test can reopen it read-only.
    fn database() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hub.db");
        let conn = Connection::open(&path).unwrap();
        prepare(&conn).unwrap();
        (dir, path)
    }

    fn opener(path: &Path) -> impl Fn() -> Result<Connection> + '_ {
        move || Ok(Connection::open(path)?)
    }

    fn read_only(path: &Path) -> impl Fn() -> Result<Connection> + '_ {
        move || {
            Ok(Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?)
        }
    }

    fn sources(answered: &[&str], failed: &[&str]) -> SourceOutcomes {
        SourceOutcomes::new(
            answered.iter().map(|name| (*name).to_string()).collect(),
            failed
                .iter()
                .map(|name| SourceFailure::new(*name, &anyhow::anyhow!("down"), &no_secrets()))
                .collect(),
        )
    }

    fn pass_at(seconds: i64, outcome: PassOutcome) -> Pass {
        Pass {
            started_at: DateTime::from_timestamp(seconds, 0).unwrap(),
            duration: Duration::from_millis(6412),
            pid: 4242,
            outcome,
        }
    }

    fn wrote(seconds: i64) -> Pass {
        pass_at(
            seconds,
            PassOutcome::Wrote {
                items: 1,
                sources: sources(&["a"], &[]),
            },
        )
    }

    /// A payload as a refresh with `items` signals and no failures produces it.
    fn payload(items: usize) -> FreshPayload {
        let refresh = workflows::status::Refresh {
            report: workflows::status::StatusReport {
                items: (0..items).map(|_| ci_failure()).collect(),
                errors: vec![],
            },
            sources: sources(&["a"], &[]),
        };
        let RefreshOutcome::Refreshed(fresh) = classify(refresh) else {
            panic!("a refresh with no failures is cached")
        };
        fresh.payload().unwrap()
    }

    fn ci_failure() -> workflows::status::StatusItem {
        workflows::status::StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("owner", "repo"),
            workflow_name: "CI".to_string(),
            job_name: None,
            step_name: None,
            error: None,
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/owner/repo/actions/runs/1".to_string(),
        })
    }

    /// How many signals the cached payload holds.
    fn cached_items(path: &Path) -> usize {
        let report: workflows::status::StatusReport =
            serde_json::from_str(&cached(path).payload).unwrap();
        report.items.len()
    }

    fn health(path: &Path) -> store::daemon_health::RecordedPass {
        store::daemon_health::recorded(&Connection::open(path).unwrap())
            .unwrap()
            .expect("a pass was recorded")
    }

    fn cached(path: &Path) -> store::status_cache::CachedStatus {
        store::status_cache::read(&Connection::open(path).unwrap())
            .unwrap()
            .expect("the cache has a row")
    }

    #[test]
    fn a_refresh_that_reached_no_source_leaves_the_previous_row_untouched() {
        let (_dir, path) = database();
        let _ = record(
            opener(&path),
            &wrote(1_000),
            Some(&payload(1)),
            &no_secrets(),
        );
        let before = cached(&path);

        let recording = record(
            opener(&path),
            &pass_at(
                2_000,
                PassOutcome::Kept {
                    sources: sources(&[], &["a"]),
                },
            ),
            None,
            &no_secrets(),
        );

        assert_eq!(recording, Recording::Recorded);
        assert_eq!(health(&path).started_at, "1970-01-01T00:33:20+00:00");
        assert_eq!(health(&path).outcome, "no_source_answered");
        let after = cached(&path);
        assert_eq!(after.payload, before.payload);
        assert_eq!(after.schema_version, before.schema_version);
        assert_eq!(after.refreshed_at, before.refreshed_at);
    }

    #[test]
    fn a_payload_the_database_refuses_leaves_a_failed_pass_saying_why_and_the_cache_as_it_was() {
        let (_dir, path) = database();
        let _ = record(
            opener(&path),
            &wrote(1_000),
            Some(&payload(1)),
            &no_secrets(),
        );
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER refuse_payload BEFORE UPDATE ON status_cache
                 BEGIN SELECT RAISE(ABORT, 'payload refused'); END;",
            )
            .unwrap();

        let recording = record(
            opener(&path),
            &wrote(2_000),
            Some(&payload(2)),
            &no_secrets(),
        );

        assert!(
            matches!(&recording, Recording::RecordedAsFailed { error }
                if error.reason().as_str().contains("payload refused")),
            "{recording:?}"
        );
        let row = health(&path);
        assert_eq!(row.outcome, "pass_failed");
        assert_eq!(row.started_at, "1970-01-01T00:33:20+00:00");
        assert!(row.error.unwrap().contains("payload refused"));
        assert_eq!(cached_items(&path), 1);
    }

    #[test]
    fn a_database_that_refuses_every_write_keeps_the_last_record_and_reports_both_errors() {
        let (_dir, path) = database();
        let _ = record(
            opener(&path),
            &wrote(1_000),
            Some(&payload(1)),
            &no_secrets(),
        );

        let recording = record(
            read_only(&path),
            &wrote(2_000),
            Some(&payload(2)),
            &no_secrets(),
        );

        assert!(
            matches!(
                &recording,
                Recording::Unrecorded {
                    fallback_error: Some(_),
                    ..
                }
            ),
            "{recording:?}"
        );
        assert_eq!(health(&path).started_at, "1970-01-01T00:16:40+00:00");
        assert_eq!(cached_items(&path), 1);
    }

    #[test]
    fn a_pass_that_had_already_failed_is_not_recorded_twice() {
        let (_dir, path) = database();
        let failed = pass_at(
            2_000,
            PassOutcome::Failed {
                failure: PassFailure::new(&anyhow::anyhow!("serialize"), &no_secrets()),
            },
        );

        let recording = record(read_only(&path), &failed, None, &no_secrets());

        assert!(
            matches!(
                &recording,
                Recording::Unrecorded {
                    fallback_error: None,
                    ..
                }
            ),
            "{recording:?}"
        );
    }

    #[test]
    fn a_credential_the_refresh_was_given_is_redacted_from_a_pass_failure() {
        let params = workflows::status::StatusParams {
            github_token: Secret::new("ghp-token-value".to_string()),
            github_username: "me".to_string(),
            pr_repos: vec![],
            issue_repos: vec![],
            ci_repos: vec![],
            linear_token: None,
            private_workflow_names: vec![],
            loki_envs: vec![],
            gcp_envs: vec![],
            extra_credentials: std::collections::HashMap::new(),
        };
        let secrets = workflows::status::known_secrets(&params);
        let refusing = || -> Result<Connection> {
            anyhow::bail!("could not open the database for ghp-token-value")
        };

        let recording = record(refusing, &wrote(2_000), Some(&payload(2)), &secrets);

        let Recording::Unrecorded {
            error,
            fallback_error: Some(fallback_error),
        } = recording
        else {
            panic!("{recording:?}")
        };
        assert!(!error.reason().as_str().contains("ghp-token-value"));
        assert!(!fallback_error.reason().as_str().contains("ghp-token-value"));
    }
}
