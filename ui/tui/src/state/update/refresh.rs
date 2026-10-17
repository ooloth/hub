use anyhow::Result;
use chrono::Utc;
use workflows::freshness::{classify, RefreshOutcome};
use workflows::status::Refresh;

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
