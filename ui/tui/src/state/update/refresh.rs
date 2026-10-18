use anyhow::{Context, Result};
use chrono::Utc;
use workflows::freshness::{classify, RefreshOutcome};
use workflows::status::{Refresh, SCHEMA_VERSION};

use crate::display::{build_unified, flatten};
use crate::state::{App, Effect, RefreshState, Screen};

pub(super) fn refresh_screen_in_place(screen: &mut Screen, raw: &[workflows::status::StatusItem]) {
    match screen {
        Screen::UnifiedList {
            items,
            flat_rows,
            selected,
            filter,
            expanded_groups,
            ..
        } => {
            let new_items = build_unified(raw.to_vec(), filter);
            *selected = (*selected).min(new_items.len().saturating_sub(1));
            *flat_rows = flatten(&new_items, expanded_groups);
            *items = new_items;
        }
        Screen::MergingPr { parent, .. } => {
            let new_items = build_unified(raw.to_vec(), &parent.filter);
            parent.selected = parent.selected.min(new_items.len().saturating_sub(1));
            parent.items = new_items;
        }
    }
}

pub(super) fn apply_report(
    app: &mut App,
    report: workflows::status::StatusReport,
    refreshed_at: chrono::DateTime<Utc>,
) {
    app.data.last_updated = Some(refreshed_at);
    app.data.refresh_state = if report.errors.is_empty() {
        RefreshState::Idle
    } else {
        RefreshState::Partial(report.errors)
    };
    app.data.raw_items = report.items;
    refresh_screen_in_place(&mut app.ui.screen, &app.data.raw_items);
}

/// Shows a refresh and caches it, unless it came back with nothing worth caching. Then the
/// earlier signals and their time stay on screen, and only the failed sources are named.
pub(super) fn apply_refresh(app: &mut App, fetched: Refresh) -> Result<Vec<Effect>> {
    match classify(fetched) {
        RefreshOutcome::Refreshed(fresh) => {
            let payload = fresh.payload()?;
            apply_report(app, fresh.into_report(), Utc::now());
            Ok(vec![Effect::WriteCache(payload)])
        }
        RefreshOutcome::NothingRefreshed { sources } => {
            app.data.refresh_state = RefreshState::Partial(
                sources
                    .failed()
                    .iter()
                    .map(|failure| failure.source().to_string())
                    .collect(),
            );
            Ok(vec![])
        }
    }
}

/// Takes in what a read of the database found: the daemon's record always, and the cached
/// signals when they are newer than the ones on screen. A read that failed leaves everything on
/// screen and keeps the reason until a read succeeds.
///
/// # Errors
/// If cached signals of this build's schema version do not parse, which only a bug can cause.
pub(super) fn apply_database_read(
    app: &mut App,
    read: Result<store::snapshot::Snapshot, String>,
) -> Result<()> {
    let snapshot = match read {
        Ok(snapshot) => snapshot,
        Err(reason) => {
            app.data.database_error = Some(reason);
            return Ok(());
        }
    };
    app.data.database_error = None;
    app.data.daemon = snapshot.daemon;
    let Some(cached) = snapshot.cached else {
        return Ok(());
    };
    let newer = app
        .data
        .last_updated
        .is_none_or(|shown| cached.refreshed_at > shown);
    if cached.schema_version != SCHEMA_VERSION || !newer {
        return Ok(());
    }
    let report: workflows::status::StatusReport = serde_json::from_str(&cached.payload)
        .context("failed to deserialize the cached signals")?;
    // The TUI's own refresh is still running, and says so until it finishes.
    if matches!(app.data.refresh_state, RefreshState::InProgress) {
        app.data.last_updated = Some(cached.refreshed_at);
        app.data.raw_items = report.items;
        refresh_screen_in_place(&mut app.ui.screen, &app.data.raw_items);
    } else {
        apply_report(app, report, cached.refreshed_at);
    }
    Ok(())
}
