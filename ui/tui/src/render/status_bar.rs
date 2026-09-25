use crate::display::FlatRow;
use crate::state::{
    compute_investigate_action, App, DetailMode, InvestigateAction, RefreshState, Screen,
};
use chrono::Utc;
use domain::profile::Profile;

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

pub(crate) const fn investigate_hint(investigate: &InvestigateAction) -> &'static str {
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
                    "{pos} · [↩] details · [p] prs · [O] issues · [e] errors · [/] search{inv_hint}{group_hint}"
                )
            }
            DetailMode::Visible { .. } => {
                let item_kind = app.current_screen().selected_item_kind();
                match item_kind {
                    crate::display::SelectedItemKind::Pr => format!(
                        "{pos} · [o] open · [d] diff · [v] review · [m] merge · [i] ask · [Esc] back"
                    ),
                    crate::display::SelectedItemKind::Issue => format!(
                        "{pos} · [o] open · [a] approve · [i] investigate · [Esc] back"
                    ),
                    crate::display::SelectedItemKind::Other => {
                        let inv_hint = investigate_hint(&inv);
                        format!("{pos} · [o] open{inv_hint} · [Esc] back")
                    }
                }
            }
        }
    } else {
        String::new()
    }
}

/// The right-hand status text, led by the profile when it is not the default
/// one.
///
/// The default profile is left unnamed: it is what every installed run uses, so
/// naming it would grow this block on every run without telling anyone
/// anything. `mod.rs` sizes the right-hand area from this string and gives the
/// left side `Constraint::Min(0)`, so every character here costs a character of
/// keybinding hints on a narrow terminal.
pub(crate) fn right_status_text(
    profile: Profile,
    state: &RefreshState,
    last_updated: Option<chrono::DateTime<Utc>>,
    now: chrono::DateTime<Utc>,
) -> String {
    let status = refresh_status_text(state, last_updated, now);
    if profile == Profile::Default {
        return status;
    }
    if status.is_empty() {
        return profile.to_string();
    }
    format!("{profile} · {status}")
}

/// What the refresh is doing, with no profile in it.
fn refresh_status_text(
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
