use crate::display::{FlatRow, SelectedItemKind};
use crate::state::{
    compute_investigate_action, App, DetailMode, InvestigateAction, RefreshState, Screen,
};
use chrono::Utc;

pub(crate) fn position_label(screen: &Screen) -> String {
    match screen {
        Screen::UnifiedList {
            items, selected, ..
        } => {
            let n = items.len();
            if n == 0 {
                String::new()
            } else {
                format!("{}/{n}", selected + 1)
            }
        }
        Screen::MergingPr { .. } => String::new(),
    }
}

/// Builds the task status submenu hint, listing all statuses except the current one.
pub(crate) fn task_status_hints(current: domain::TaskStatus) -> String {
    const ENTRIES: &[(char, domain::TaskStatus, &str)] = &[
        ('b', domain::TaskStatus::Backlog, "backlog"),
        ('l', domain::TaskStatus::Blocked, "blocked"),
        ('c', domain::TaskStatus::Cancelled, "cancelled"),
        ('d', domain::TaskStatus::Done, "done"),
        ('f', domain::TaskStatus::Failed, "failed"),
        ('i', domain::TaskStatus::InProgress, "in-progress"),
        ('v', domain::TaskStatus::InReview, "in-review"),
        ('r', domain::TaskStatus::Ready, "ready"),
    ];
    let mut parts: Vec<String> = ENTRIES
        .iter()
        .filter(|(_, s, _)| *s != current)
        .map(|(k, _, name)| format!("[{k}] {name}"))
        .collect();
    parts.push("[Esc] cancel".to_string());
    format!("  {}", parts.join(" · "))
}

pub(crate) fn investigate_hint(investigate: &InvestigateAction) -> &'static str {
    if matches!(investigate, InvestigateAction::None) {
        ""
    } else {
        " · [i] investigate"
    }
}

/// Returns the number of wrapped lines the body text produces at the given width.
/// Used to clamp scroll in the issue reader.
pub(crate) fn issue_body_line_count(body: Option<&str>, width: usize) -> usize {
    let text = body.unwrap_or("");
    if text.is_empty() {
        return 1; // placeholder "(no description)" is one line
    }
    text.lines()
        .map(|line| {
            if line.is_empty() {
                1
            } else {
                super::unified::wrap_text(line, width).len()
            }
        })
        .sum()
}

pub(crate) fn status_bar_left(app: &App) -> String {
    if let Some(flash) = &app.ui.flash {
        return flash.clone();
    }
    if let Screen::UnifiedList {
        flat_rows,
        selected,
        detail_mode,
        ..
    } = &app.ui.screen
    {
        let pos = position_label(app.current_screen());
        let inv = compute_investigate_action(app);
        match detail_mode {
            DetailMode::Hidden => {
                let item_kind = app.current_screen().selected_item_kind();
                let task_hint = if matches!(
                    item_kind,
                    SelectedItemKind::Task | SelectedItemKind::BadgedSignal
                ) {
                    " · [s] status"
                } else {
                    ""
                };
                let inv_hint = investigate_hint(&inv);
                let group_hint = match flat_rows.get(*selected) {
                    Some(FlatRow::GroupHeader {
                        expanded: false, ..
                    }) => " · [l] expand",
                    Some(
                        FlatRow::GroupHeader { expanded: true, .. } | FlatRow::GroupChild { .. },
                    ) => " · [h] collapse",
                    _ => "",
                };
                format!(
                    "{pos} · [↩] details{task_hint} · [n/N] new task · [p] prs · [O] issues · [e] errors · [/] search{inv_hint}{group_hint}"
                )
            }
            DetailMode::Visible { .. } => {
                let item_kind = app.current_screen().selected_item_kind();
                match item_kind {
                    SelectedItemKind::Pr => format!(
                        "{pos} · [o] open · [d] diff · [v] review · [m] merge · [i] ask · [Esc] back"
                    ),
                    SelectedItemKind::Issue => format!(
                        "{pos} · [o] open · [a] approve · [i] investigate · [Esc] back"
                    ),
                    SelectedItemKind::Task => format!("{pos} · [s] status · [Esc] back"),
                    SelectedItemKind::BadgedSignal => {
                        format!("{pos} · [s] status · [Tab] session · [Esc] back")
                    }
                    SelectedItemKind::Other => {
                        let inv_hint = investigate_hint(&inv);
                        format!("{pos} · [o] open{inv_hint} · [Esc] back")
                    }
                }
            }
            DetailMode::VisibleSession { .. } => {
                format!("{pos} · [Tab] signal detail · [Esc] back")
            }
        }
    } else {
        String::new()
    }
}

pub(crate) fn right_status_text(
    state: &RefreshState,
    last_updated: Option<chrono::DateTime<Utc>>,
    now: chrono::DateTime<Utc>,
) -> String {
    let age_str = |t: chrono::DateTime<Utc>| {
        let mins = (now - t).num_minutes();
        if mins == 0 {
            "just now".to_string()
        } else {
            format!("{mins}m ago")
        }
    };
    match state {
        RefreshState::InProgress => "refreshing…".to_string(),
        RefreshState::Partial(failed_sources) => {
            let time_str = last_updated.map_or_else(|| "unknown".to_string(), age_str);
            let sources = failed_sources.join(", ");
            format!("! {sources} unreachable (updated {time_str})")
        }
        RefreshState::Failed(err) => format!("refresh failed: {err}"),
        RefreshState::Idle => last_updated
            .map(|t| format!("updated {}", age_str(t)))
            .unwrap_or_default(),
    }
}
