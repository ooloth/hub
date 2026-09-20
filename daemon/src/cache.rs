//! Writing a refreshed status into hub's status cache.

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::freshness::{FreshStatus, RefreshOutcome};

/// Replaces the cache row with a refreshed status.
///
/// Takes a [`FreshStatus`], so a refresh that came back with nothing cannot
/// reach this function.
///
/// # Errors
/// Returns an error if the report cannot be serialized or the write fails.
pub(crate) fn write(conn: &Connection, fresh: &FreshStatus) -> Result<()> {
    let payload = fresh.payload()?;
    store::status_cache::upsert(conn, &payload, workflows::status::SCHEMA_VERSION)
        .context("failed to write the status cache")
}

/// Applies a refresh outcome to the cache: writes on
/// [`RefreshOutcome::Refreshed`], leaves the existing row alone on
/// [`RefreshOutcome::NothingRefreshed`].
///
/// # Errors
/// Returns an error if the write fails.
pub(crate) fn apply(conn: &Connection, outcome: &RefreshOutcome) -> Result<()> {
    match outcome {
        RefreshOutcome::Refreshed(fresh) => write(conn, fresh),
        RefreshOutcome::NothingRefreshed { .. } => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::freshness::classify;
    use workflows::status::{StatusItem, StatusReport};

    fn in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        store::status_cache::ensure_table(&conn).unwrap();
        conn
    }

    fn ci_failure() -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
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

    fn refreshed(items: usize, failed_sources: usize) -> RefreshOutcome {
        classify(StatusReport {
            items: (0..items).map(|_| ci_failure()).collect(),
            errors: (0..failed_sources).map(|i| format!("source {i}")).collect(),
        })
    }

    fn nothing_refreshed() -> RefreshOutcome {
        classify(StatusReport {
            items: vec![],
            errors: vec!["github prs".to_string()],
        })
    }

    #[test]
    fn a_cached_refresh_reads_back_as_the_same_status_report() {
        let conn = in_memory();
        let RefreshOutcome::Refreshed(fresh) = refreshed(3, 1) else {
            panic!("a refresh that reached a source must be cached")
        };

        write(&conn, &fresh).unwrap();

        let cached = store::status_cache::read(&conn).unwrap().unwrap();
        let round_tripped: StatusReport = serde_json::from_str(&cached.payload).unwrap();
        assert_eq!(round_tripped.items.len(), 3);
        assert_eq!(round_tripped.errors, vec!["source 0"]);
    }

    #[test]
    fn a_cached_refresh_is_stamped_with_the_schema_version_the_tui_reads() {
        let conn = in_memory();
        let RefreshOutcome::Refreshed(fresh) = refreshed(1, 0) else {
            panic!("a refresh that reached a source must be cached")
        };

        write(&conn, &fresh).unwrap();

        let cached = store::status_cache::read(&conn).unwrap().unwrap();
        assert_eq!(cached.schema_version, workflows::status::SCHEMA_VERSION);
    }

    #[test]
    fn a_refresh_that_reached_no_source_leaves_the_previous_row_untouched() {
        let conn = in_memory();
        apply(&conn, &refreshed(2, 0)).unwrap();
        let before = store::status_cache::read(&conn).unwrap().unwrap();

        apply(&conn, &nothing_refreshed()).unwrap();

        let after = store::status_cache::read(&conn).unwrap().unwrap();
        assert_eq!(after.payload, before.payload);
        assert_eq!(after.schema_version, before.schema_version);
        assert_eq!(after.refreshed_at, before.refreshed_at);
    }
}
