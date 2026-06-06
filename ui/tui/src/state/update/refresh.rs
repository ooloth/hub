use anyhow::{Context, Result};
use chrono::Utc;

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

/// Replaces every agent-session (task) item in `raw` with `fresh_tasks`, leaving
/// all other signal sources (PRs, CI, Loki, …) untouched.
///
/// The 10s task poll re-reads task state from SQLite and patches it into the
/// in-memory list so an agent's `hub task report` is reflected within seconds,
/// without waiting for the full 30-minute external refresh. Tasks are appended
/// at the end; `build_unified` re-sorts by urgency, so position here is moot.
pub(crate) fn patch_task_items(
    raw: Vec<workflows::status::StatusItem>,
    fresh_tasks: Vec<domain::Task>,
) -> Vec<workflows::status::StatusItem> {
    use workflows::status::StatusItem;
    let mut patched: Vec<StatusItem> = raw
        .into_iter()
        .filter(|i| !matches!(i, StatusItem::AgentSession(_)))
        .collect();
    patched.extend(fresh_tasks.into_iter().map(StatusItem::AgentSession));
    patched
}

/// Patches freshly-read tasks into the in-memory list and rebuilds the screen.
pub(super) fn patch_tasks(app: &mut App, fresh_tasks: Vec<domain::Task>) {
    let raw = std::mem::take(&mut app.data.raw_items);
    app.data.raw_items = patch_task_items(raw, fresh_tasks);
    refresh_screen_in_place(&mut app.ui.screen, &app.data.raw_items);
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

pub(super) fn apply_refresh(
    app: &mut App,
    report: workflows::status::StatusReport,
) -> Result<Vec<Effect>> {
    let json = serde_json::to_string(&report).context("failed to serialize status report")?;
    apply_report(app, report, Utc::now());
    Ok(vec![Effect::WriteCache(json)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use workflows::status::StatusItem;

    fn task(id: &str) -> domain::Task {
        domain::Task {
            id: id.parse().unwrap(),
            title: format!("Task {id}"),
            description: None,
            status: domain::TaskStatus::InProgress,
            kind: domain::TaskKind::Implement,
            session_id: None,
            repo: None,
            links: vec![],
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            comments: vec![],
        }
    }

    fn gcp_item(message: &str) -> StatusItem {
        StatusItem::Gcp(domain::GcpEntry {
            title: "t".into(),
            project: "p".into(),
            env: "e".into(),
            message: message.into(),
            line: "{}".into(),
            lookback: "1h".into(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            url: "https://example.com".into(),
            gcp_project: "gp".into(),
        })
    }

    fn task_ids(items: &[StatusItem]) -> Vec<String> {
        items
            .iter()
            .filter_map(|i| match i {
                StatusItem::AgentSession(t) => Some(t.id.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn patch_task_items_replaces_tasks_with_fresh_list() {
        let raw = vec![
            StatusItem::AgentSession(task("TASK-0001")),
            StatusItem::AgentSession(task("TASK-0002")),
        ];
        let patched = patch_task_items(raw, vec![task("TASK-0003")]);
        assert_eq!(
            task_ids(&patched),
            vec!["TASK-0003"],
            "stale task items are replaced by the fresh list"
        );
    }

    #[test]
    fn patch_task_items_preserves_non_task_items() {
        let raw = vec![
            gcp_item("alpha"),
            StatusItem::AgentSession(task("TASK-0001")),
        ];
        let patched = patch_task_items(raw, vec![]);
        assert_eq!(
            patched
                .iter()
                .filter(|i| matches!(i, StatusItem::Gcp(_)))
                .count(),
            1,
            "non-task signals survive a task patch"
        );
        assert!(
            task_ids(&patched).is_empty(),
            "tasks are cleared when the fresh list is empty"
        );
    }
}
