use crate::display::FlatRow;
use crate::state::{selected_launch, App, DetailMode, RefreshState, Screen, SubmenuState};
use chrono::Utc;
use domain::daemon_health::{DaemonRecord, DaemonStatus};
use domain::profile::Profile;

/// One key the status bar offers, and what pressing it does.
///
/// The key and the label stay apart until `hint_spans` draws them, so the key
/// can take the list title's style while the label stays dim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KeyHint {
    pub(crate) key: String,
    pub(crate) label: String,
}

impl KeyHint {
    pub(crate) fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }
}

/// What the left side of the status bar shows.
///
/// An open submenu or the merge question wins over a flash message, and a flash
/// message wins over the keys the screen itself offers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StatusLeft {
    Flash(String),
    /// Keys to offer, after a prompt saying what they act on when a submenu
    /// or the merge question is open.
    Hints {
        prompt: Option<String>,
        hints: Vec<KeyHint>,
    },
}

/// Offered only when the selected signal has a launch.
fn investigate_hint() -> KeyHint {
    KeyHint::new("i", "investigate")
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

pub(crate) fn status_bar_left(app: &App) -> StatusLeft {
    if let Some((prompt, hints)) = prompt_hints(app) {
        return StatusLeft::Hints {
            prompt: Some(prompt),
            hints,
        };
    }
    if let Some(flash) = &app.ui.flash {
        return StatusLeft::Flash(flash.clone());
    }
    StatusLeft::Hints {
        prompt: None,
        hints: screen_hints(app),
    }
}

/// The prompt and keys of an open submenu or of the merge question.
fn prompt_hints(app: &App) -> Option<(String, Vec<KeyHint>)> {
    if app.ui.submenu == SubmenuState::PrActions {
        let prompt = match app.current_screen().selected_status_item() {
            Some(workflows::status::StatusItem::Pr(pr)) => format!("PR #{} · diff", pr.number),
            _ => "diff".to_string(),
        };
        let hints = vec![
            KeyHint::new("d", "delta"),
            KeyHint::new("l", "lazygit"),
            KeyHint::new("o", "octo"),
            KeyHint::new("Esc", "cancel"),
        ];
        return Some((prompt, hints));
    }
    if let SubmenuState::ReviewPicker(target) = &app.ui.submenu {
        // Options come from the same table the key handler resolves against, so
        // the bar cannot advertise a key that does nothing.
        let hints = target
            .ownership
            .review_options()
            .iter()
            .map(|option| KeyHint::new(option.key, option.label))
            .chain([KeyHint::new("Esc", "cancel")])
            .collect();
        return Some((format!("Review #{}", target.number), hints));
    }
    if let Screen::MergingPr { pr, .. } = &app.ui.screen {
        let question = format!("Squash and merge #{} into {}?", pr.number, pr.base_branch);
        let hints = vec![KeyHint::new("↩", "confirm"), KeyHint::new("Esc", "cancel")];
        return Some((question, hints));
    }
    None
}

/// The keys the list or the open detail pane offers.
fn screen_hints(app: &App) -> Vec<KeyHint> {
    let Screen::UnifiedList {
        flat_rows,
        selected,
        detail_mode,
        ..
    } = &app.ui.screen
    else {
        return vec![];
    };
    let launch = selected_launch(app);
    match detail_mode {
        DetailMode::Hidden => {
            let group_hint = match flat_rows.get(*selected) {
                Some(FlatRow::GroupHeader {
                    expanded: false, ..
                }) => Some(KeyHint::new("l", "expand")),
                Some(FlatRow::GroupHeader { expanded: true, .. } | FlatRow::GroupChild { .. }) => {
                    Some(KeyHint::new("h", "collapse"))
                }
                _ => None,
            };
            [
                Some(KeyHint::new("↩", "details")),
                Some(KeyHint::new("p", "prs")),
                Some(KeyHint::new("O", "issues")),
                Some(KeyHint::new("e", "errors")),
                Some(KeyHint::new("/", "search")),
                launch.is_some().then(investigate_hint),
                group_hint,
            ]
            .into_iter()
            .flatten()
            .collect()
        }
        DetailMode::Visible { .. } => match app.current_screen().selected_item_kind() {
            crate::display::SelectedItemKind::Pr => vec![
                KeyHint::new("o", "open"),
                KeyHint::new("d", "diff"),
                KeyHint::new("v", "review"),
                KeyHint::new("m", "merge"),
                KeyHint::new("i", "ask"),
                KeyHint::new("Esc", "back"),
            ],
            crate::display::SelectedItemKind::Issue => vec![
                KeyHint::new("o", "open"),
                KeyHint::new("a", "approve"),
                KeyHint::new("i", "investigate"),
                KeyHint::new("Esc", "back"),
            ],
            crate::display::SelectedItemKind::Other => [
                Some(KeyHint::new("o", "open")),
                launch.is_some().then(investigate_hint),
                Some(KeyHint::new("Esc", "back")),
            ]
            .into_iter()
            .flatten()
            .collect(),
        },
    }
}

/// The right-hand status text, led by the profile when it is not the default
/// one.
///
/// The default profile is left unnamed: it is what every installed run uses, so
/// naming it would grow this block on every run without telling anyone
/// anything. `mod.rs` sizes the right-hand area from `right_text`, which starts
/// with this string, and gives the
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
            let time_str = match last_updated {
                Some(updated) => age_str(updated),
                None => "unknown".to_string(),
            };
            let sources = failed_sources.join(", ");
            format!("! {sources} unreachable (updated {time_str})")
        }
        RefreshState::Failed(err) => format!("refresh failed: {err}"),
        RefreshState::Idle => match last_updated {
            Some(t) => format!("updated {}", age_str(t)),
            None => String::new(),
        },
    }
}

/// The whole right-hand status text: the profile and the refresh, then a warning when the
/// daemon's refreshes have stopped reaching the signals or the notifications.
pub(crate) fn right_text(app: &App, now: chrono::DateTime<Utc>) -> String {
    let status = right_status_text(
        app.profile,
        &app.data.refresh_state,
        app.data.last_updated,
        now,
    );
    let warning = daemon_warning(&app.data.daemon, app.data.database_error.as_deref(), now);
    match (status.is_empty(), warning) {
        (_, None) => status,
        (true, Some(warning)) => warning,
        (false, Some(warning)) => format!("{status} · {warning}"),
    }
}

/// What the status bar says about background refreshes, or nothing while they run on schedule.
///
/// Each warning names what stopped rather than the daemon, which is an implementation detail
/// until it affects the signals or the notifications.
pub(crate) fn daemon_warning(
    daemon: &DaemonRecord,
    database_error: Option<&str>,
    now: chrono::DateTime<Utc>,
) -> Option<String> {
    if let Some(error) = database_error {
        return Some(format!("! can't read hub.db: {error}"));
    }
    match daemon.status(now) {
        DaemonStatus::Current => None,
        DaemonStatus::Overdue { age } => Some(format!(
            "! no refresh in {}, notifications paused",
            whole_minutes_or_hours(age)
        )),
        DaemonStatus::WaitingForCredentials => {
            Some("! refresh waiting for 1Password, notifications paused".to_string())
        }
        DaemonStatus::NoneRecorded => {
            Some("! no background refresh yet, notifications off".to_string())
        }
        DaemonStatus::RanOnce => {
            Some("! no background refresh scheduled, notifications off".to_string())
        }
        DaemonStatus::OtherVersion => {
            Some("! can't read refresh status (another hub version)".to_string())
        }
    }
}

/// `47m` under an hour, and `3h` from an hour on, rounded down.
fn whole_minutes_or_hours(age: std::time::Duration) -> String {
    let minutes = age.as_secs() / 60;
    if minutes < 60 {
        format!("{minutes}m")
    } else {
        format!("{}h", minutes / 60)
    }
}
