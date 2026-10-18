//! Reading the cached signals and the daemon's health record together, which is the only way to
//! read either (Decision 026).
use anyhow::{Context, Result};
use domain::daemon_health::DaemonRecord;
use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::status_cache::{self, CachedStatus};

/// The cached signals and the daemon's last record, from one moment of the database.
#[derive(Debug)]
pub struct Snapshot {
    /// The cached signals, if any were ever written.
    pub cached: Option<CachedStatus>,
    /// What the daemon last recorded.
    pub daemon: DaemonRecord,
}

/// Reads the cached signals and the daemon's last record in one read transaction, so both come
/// from the same moment even while the daemon is writing.
///
/// A health table this build did not create is reported as [`DaemonRecord::OtherVersion`] and
/// left as it is: the daemon that wrote it recreates it on its own terms.
///
/// # Errors
/// Returns an error if the database cannot be read, or holds a row this build's schema does not
/// allow.
pub fn read(conn: &Connection) -> Result<Snapshot> {
    // Deferred: the first SELECT fixes the snapshot every later statement in it reads.
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Deferred)
        .context("failed to begin reading the database")?;
    let cached = status_cache::read(&transaction)?;
    let daemon = crate::daemon_health::last_record(&transaction)?;
    transaction
        .commit()
        .context("failed to finish reading the database")?;
    Ok(Snapshot { cached, daemon })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::DateTime;
    use domain::credential_attempt::CredentialAttempt;
    use domain::credential_failure::CredentialFailure;
    use domain::daemon_health::{DaemonHealth, LastActivity, Schedule};
    use domain::daemon_pass::{Pass, PassOutcome, PassStatus};
    use domain::known_secrets::KnownSecrets;
    use domain::source_failure::SourceFailure;
    use domain::source_outcomes::SourceOutcomes;
    use rstest::rstest;

    use super::*;

    fn in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        status_cache::ensure_table(&conn).unwrap();
        crate::daemon_health::ensure_table(&conn).unwrap();
        conn
    }

    fn no_secrets() -> KnownSecrets {
        KnownSecrets::new(Vec::new())
    }

    fn started_at() -> DateTime<chrono::Utc> {
        DateTime::from_timestamp(1_760_000_000, 0).unwrap()
    }

    /// A pass that kept the payload, so it is recorded without one. `failed` is never empty.
    fn pass(answered: &[&str], failed: &[&str]) -> Pass {
        Pass {
            started_at: started_at(),
            duration: Duration::from_millis(4100),
            pid: 4242,
            outcome: PassOutcome::Kept {
                sources: SourceOutcomes::new(
                    answered.iter().map(|name| (*name).to_string()).collect(),
                    failed
                        .iter()
                        .map(|name| {
                            SourceFailure::new(*name, &anyhow::anyhow!("down"), &no_secrets())
                        })
                        .collect(),
                ),
            },
        }
    }

    fn credential_attempt() -> CredentialAttempt {
        CredentialAttempt {
            started_at: started_at(),
            duration: Duration::from_millis(812),
            pid: 4242,
            attempt: 1,
            failure: CredentialFailure::new(&anyhow::anyhow!("dial tcp"), &no_secrets()),
            opened_app: None,
            notified: None,
            retry_in: Some(Duration::from_secs(1)),
        }
    }

    #[test]
    fn an_empty_database_reads_as_no_signals_and_no_record() {
        let snapshot = read(&in_memory()).unwrap();

        assert!(snapshot.cached.is_none());
        assert_eq!(snapshot.daemon, DaemonRecord::NoneRecorded);
    }

    #[test]
    fn signals_without_a_record_read_as_signals_and_no_record() {
        let conn = in_memory();
        status_cache::upsert(&conn, r#"{"items":[]}"#, 18).unwrap();

        let snapshot = read(&conn).unwrap();

        let cached = snapshot.cached.expect("the signals were cached");
        assert_eq!(cached.payload, r#"{"items":[]}"#);
        assert_eq!(cached.schema_version, 18);
        assert_eq!(snapshot.daemon, DaemonRecord::NoneRecorded);
    }

    #[rstest]
    #[case::every_twenty_seconds(
        Schedule::every(Duration::from_secs(20)),
        pass(&["a"], &["b"]),
        LastActivity::Pass(PassStatus::Partial)
    )]
    #[case::once(
        Schedule::Once,
        pass(&[], &["a"]),
        LastActivity::Pass(PassStatus::NoSourceAnswered)
    )]
    fn a_pass_reads_back_with_its_schedule_and_how_it_ended(
        #[case] schedule: Schedule,
        #[case] pass: Pass,
        #[case] activity: LastActivity,
    ) {
        let conn = in_memory();
        crate::daemon_health::record(&conn, &pass, schedule, None).unwrap();

        let snapshot = read(&conn).unwrap();

        assert_eq!(
            snapshot.daemon,
            DaemonRecord::Recorded(DaemonHealth {
                started_at: started_at(),
                schedule,
                activity,
            })
        );
    }

    #[test]
    fn a_credential_attempt_reads_back_as_waiting_for_credentials() {
        let conn = in_memory();
        let schedule = Schedule::every(Duration::from_secs(15 * 60));
        crate::daemon_health::record_credential_attempt(&conn, &credential_attempt(), schedule)
            .unwrap();

        let snapshot = read(&conn).unwrap();

        assert_eq!(
            snapshot.daemon,
            DaemonRecord::Recorded(DaemonHealth {
                started_at: started_at(),
                schedule,
                activity: LastActivity::WaitingForCredentials,
            })
        );
    }
}
