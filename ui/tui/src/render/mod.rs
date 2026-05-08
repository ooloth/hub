use chrono::Utc;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::display::{display_item_line, display_item_urgency, DisplayItem};
use crate::state::{
    compute_enter_action, compute_investigate_action, App, EnterAction, InvestigateAction,
    RefreshState, Screen,
};

mod detail;

pub(super) const FOCUS_COLOR: Color = Color::Green;
pub(super) const SELECTION_BG: Color = Color::Rgb(41, 45, 62);

pub(super) fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub(super) fn list_highlight() -> Style {
    Style::default()
        .bg(SELECTION_BG)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn urgency_style(u: domain::Urgency) -> Style {
    match u {
        domain::Urgency::Critical => Style::default().fg(Color::Red),
        domain::Urgency::High => Style::default().fg(Color::Yellow),
        domain::Urgency::Medium => Style::default(),
        domain::Urgency::Low => dim(),
    }
}

const KEYBINDS_LIST: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("h / k", "up"),
    ("j / l", "down"),
    ("Enter", "open / drill into group"),
    ("i", "investigate"),
    ("r", "refresh"),
    ("q / Ctrl-C", "quit"),
];

const KEYBINDS_DETAIL: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("h / k", "up"),
    ("j / l", "down"),
    ("Enter", "open URL"),
    ("i", "investigate"),
    ("r", "refresh"),
    ("Esc", "back to list"),
    ("q / Ctrl-C", "quit"),
];

fn format_keybinds(keybinds: &[(&str, &str)]) -> String {
    let key_w = keybinds
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    keybinds
        .iter()
        .map(|(k, d)| format!("  {k:<key_w$}   {d}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn popup_area(area: Rect, content_lines: u16, content_width: u16) -> Rect {
    let width = (content_width + 4).min(area.width);
    let height = (content_lines + 2).min(area.height);
    Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    )
}

pub(super) fn render_list_view<T>(
    frame: &mut ratatui::Frame,
    area: Rect,
    items: &[T],
    list_state: &mut ListState,
    item_data: impl Fn(&T) -> (String, Option<String>, domain::Urgency),
    hint_fn: impl Fn(&T) -> Option<String>,
) {
    let text_width = area.width.saturating_sub(2) as usize;
    let selected = list_state.selected();
    let selected_hint = selected.and_then(|i| items.get(i)).and_then(hint_fn);
    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = selected == Some(i);
            let (line_text, dim_suffix, urgency) = item_data(item);
            let dot_style = if is_selected {
                Style::default()
            } else {
                urgency_style(urgency)
            };
            let hint = if is_selected {
                selected_hint.clone()
            } else {
                None
            };
            let item_width = text_width
                .saturating_sub(dim_suffix.as_ref().map_or(0, |s| s.chars().count()))
                .saturating_sub(hint.as_ref().map_or(0, |h| h.chars().count() + 2));
            build_list_item(
                Span::styled("● ", dot_style),
                wrap_text(&line_text, item_width),
                dim_suffix,
                hint,
            )
        })
        .collect();
    let list = List::new(list_items).highlight_style(list_highlight());
    frame.render_stateful_widget(list, area, list_state);
}

fn build_list_item(
    dot: Span<'static>,
    wrapped: Vec<String>,
    dim_suffix: Option<String>,
    hint: Option<String>,
) -> ListItem<'static> {
    let suffix_span = dim_suffix.map(|s| Span::styled(s, dim()));
    let hint_span = hint.map(|h| Span::styled(format!("  {h}"), dim()));
    let mut lines: Vec<Line> = wrapped
        .into_iter()
        .enumerate()
        .map(|(j, chunk)| {
            if j == 0 {
                Line::from(vec![dot.clone(), Span::raw(chunk)])
            } else {
                Line::from(Span::raw(format!("  {chunk}")))
            }
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::from(vec![dot, Span::raw("")]));
    }
    if let Some(last) = lines.last_mut() {
        if let Some(s) = suffix_span {
            last.spans.push(s);
        }
        if let Some(h) = hint_span {
            last.spans.push(h);
        }
    }
    ListItem::new(Text::from(lines))
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    if total <= width {
        return vec![text.to_string()];
    }
    let mut lines = vec![];
    let mut start = 0;
    while start < total {
        let end = (start + width).min(total);
        if end == total {
            lines.push(chars[start..].iter().collect());
            break;
        }
        if chars[end] == ' ' {
            lines.push(chars[start..end].iter().collect());
            start = end + 1;
            while start < total && chars[start] == ' ' {
                start += 1;
            }
            continue;
        }
        let split = chars[start..end]
            .iter()
            .rposition(|&c| c == ' ')
            .filter(|&p| p > 0)
            .map(|p| start + p);
        if let Some(split) = split {
            lines.push(chars[start..split].iter().collect());
            start = split + 1;
            while start < total && chars[start] == ' ' {
                start += 1;
            }
        } else {
            lines.push(chars[start..end].iter().collect());
            start = end;
        }
    }
    lines
}

fn position_label(screen: &Screen) -> String {
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
        Screen::Detail { parent, view } => {
            let count = match parent.items.get(view.group_index) {
                Some(DisplayItem::Group { items, .. }) => items.len(),
                _ => 0,
            };
            view.list_state
                .selected()
                .map(|i| format!("{}/{count}", i + 1))
                .unwrap_or_default()
        }
    }
}

fn action_hints(enter: &EnterAction, investigate: &InvestigateAction) -> String {
    let enter_hint = match enter {
        EnterAction::OpenUrl(url) => format!(" · Press ↩ to open {url}"),
        EnterAction::OpenDetail { item_count, .. } => {
            format!(" · Press ↩ to expand ({item_count} items)")
        }
        EnterAction::None => String::new(),
    };
    let inv_hint = if matches!(investigate, InvestigateAction::None) {
        ""
    } else {
        " · Press i to investigate"
    };
    format!("{enter_hint}{inv_hint}")
}

fn status_bar_left(app: &App) -> String {
    if let Some(flash) = &app.ui.flash {
        return flash.clone();
    }
    let enter_action = compute_enter_action(app);
    let investigate_action = compute_investigate_action(app);
    let pos = position_label(app.current_screen());
    let hints = action_hints(&enter_action, &investigate_action);
    format!("{pos}{hints}")
}

// Stub render for the unified list. Reuses render_list_view with a
// ratatui ListState driven by `selected`. Step 3 will replace this
// with urgency-tier dividers and the full line format.
fn render_unified_stub(
    frame: &mut ratatui::Frame,
    items: &[DisplayItem],
    selected: usize,
    area: Rect,
) {
    let block = Block::new().borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut ls = ListState::default();
    if !items.is_empty() {
        ls.select(Some(selected));
    }
    render_list_view(
        frame,
        inner,
        items,
        &mut ls,
        |item| {
            let (line_text, dim_suffix) = match item {
                DisplayItem::Group { label, items } => {
                    (label.clone(), Some(format!(" ({})", items.len())))
                }
                DisplayItem::Single(_) => (display_item_line(item), None),
            };
            (line_text, dim_suffix, display_item_urgency(item))
        },
        |item| match item {
            DisplayItem::Group { .. } => Some("↩ to expand".to_string()),
            DisplayItem::Single(s) => crate::display::item_hint(s),
        },
    );
}

pub(crate) fn render(frame: &mut ratatui::Frame, app: &mut App) {
    let [content_area, bar_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    match &mut app.ui.screen {
        Screen::UnifiedList {
            items, selected, ..
        } => {
            render_unified_stub(frame, items, *selected, content_area);
        }
        Screen::Detail { parent, view } => {
            detail::render_detail(frame, view, parent, content_area);
        }
    }

    let right_status = match &app.data.refresh_state {
        RefreshState::InProgress => "refreshing…".to_string(),
        RefreshState::Failed(err) => format!("refresh failed: {err}"),
        RefreshState::Idle => {
            if let Some(t) = app.data.last_updated {
                let mins = (Utc::now() - t).num_minutes();
                if mins == 0 {
                    "updated just now".to_string()
                } else {
                    format!("updated {mins}m ago")
                }
            } else {
                String::new()
            }
        }
    };

    let left = status_bar_left(app);

    let right_width = right_status.chars().count() as u16;
    let [bar_left, bar_right] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(bar_area);
    frame.render_widget(Paragraph::new(left).style(dim()), bar_left);
    frame.render_widget(Paragraph::new(right_status).style(dim()), bar_right);

    if app.ui.show_help {
        let keybinds = match &app.ui.screen {
            Screen::UnifiedList { .. } => KEYBINDS_LIST,
            Screen::Detail { .. } => KEYBINDS_DETAIL,
        };
        let text = format_keybinds(keybinds);
        let lines = keybinds.len() as u16;
        let width = text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
        let popup = popup_area(frame.area(), lines, width);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(text).block(Block::new().title(" Keybinds ").borders(Borders::ALL)),
            popup,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{action_hints, position_label, status_bar_left, wrap_text};
    use crate::display::{CatData, Category, DisplayItem, Filter, ListSnapshot};
    use crate::state::{
        App, DataState, DetailView, EnterAction, InvestigateAction, Screen, UiState,
    };
    use ratatui::widgets::ListState;
    use workflows::status::StatusItem;

    fn pr() -> StatusItem {
        StatusItem::Pr(domain::PullRequest {
            number: 1,
            title: "Fix".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/1".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
        })
    }

    fn unified_list_app(items: Vec<DisplayItem>) -> App {
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items,
                    selected: 0,
                    filter: Filter::default(),
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn detail_app(snapshot_items: Vec<DisplayItem>, group_index: usize, sel: usize) -> App {
        let mut ls = ListState::default();
        ls.select(Some(sel));
        App {
            ui: UiState {
                screen: Screen::Detail {
                    parent: ListSnapshot {
                        items: snapshot_items,
                        selected: 0,
                        filter: Filter::default(),
                    },
                    view: DetailView {
                        group_index,
                        list_state: ls,
                    },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    #[test]
    fn status_bar_shows_flash_when_set() {
        let app = App {
            ui: UiState {
                flash: Some("something went wrong".to_string()),
                ..UiState::default()
            },
            ..App::default()
        };
        assert_eq!(status_bar_left(&app), "something went wrong");
    }

    #[test]
    fn status_bar_unified_list_shows_position() {
        let app = unified_list_app(vec![DisplayItem::Single(pr()), DisplayItem::Single(pr())]);
        assert!(status_bar_left(&app).starts_with("1/2"));
    }

    #[test]
    fn status_bar_empty_unified_list_shows_no_position() {
        let app = unified_list_app(vec![]);
        assert!(!status_bar_left(&app).starts_with("1/"));
    }

    #[test]
    fn position_label_unified_list_shows_index_of_n() {
        let screen = Screen::UnifiedList {
            items: vec![
                DisplayItem::Single(pr()),
                DisplayItem::Single(pr()),
                DisplayItem::Single(pr()),
            ],
            selected: 1,
            filter: Filter::default(),
        };
        assert_eq!(position_label(&screen), "2/3");
    }

    #[test]
    fn position_label_empty_unified_list_is_empty() {
        let screen = Screen::UnifiedList {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
        };
        assert_eq!(position_label(&screen), "");
    }

    #[test]
    fn position_label_detail_shows_index_within_group() {
        let app = detail_app(
            vec![DisplayItem::Group {
                label: "hub".to_string(),
                items: vec![pr(), pr()],
            }],
            0,
            0,
        );
        assert_eq!(position_label(app.current_screen()), "1/2");
    }

    #[test]
    fn action_hints_open_url() {
        let enter = EnterAction::OpenUrl("https://example.com".to_string());
        let inv = InvestigateAction::None;
        assert_eq!(
            action_hints(&enter, &inv),
            " · Press ↩ to open https://example.com"
        );
    }

    #[test]
    fn action_hints_expand_group() {
        let enter = EnterAction::OpenDetail {
            group_index: 0,
            item_count: 3,
        };
        let inv = InvestigateAction::None;
        assert_eq!(action_hints(&enter, &inv), " · Press ↩ to expand (3 items)");
    }

    #[test]
    fn action_hints_investigate_ci() {
        let enter = EnterAction::None;
        let inv = InvestigateAction::LaunchCi {
            repo: "owner/repo".to_string(),
            run_url: "https://example.com".to_string(),
        };
        assert_eq!(action_hints(&enter, &inv), " · Press i to investigate");
    }

    #[cfg(feature = "private")]
    #[test]
    fn action_hints_investigate_sonarr() {
        let enter = EnterAction::None;
        let inv = InvestigateAction::LaunchSonarrBlocked {
            title: "Show — S01E01".to_string(),
            error: "Invalid video file".to_string(),
        };
        assert_eq!(action_hints(&enter, &inv), " · Press i to investigate");
    }

    #[test]
    fn action_hints_combined() {
        let enter = EnterAction::OpenUrl("https://example.com".to_string());
        let inv = InvestigateAction::LaunchCi {
            repo: "owner/repo".to_string(),
            run_url: "https://example.com".to_string(),
        };
        assert_eq!(
            action_hints(&enter, &inv),
            " · Press ↩ to open https://example.com · Press i to investigate"
        );
    }

    #[test]
    fn wrap_text_treats_zero_width_as_one_column() {
        assert_eq!(wrap_text("abc", 0), vec!["a", "b", "c"]);
    }

    #[test]
    fn wrap_text_hard_wraps_without_dropping_characters() {
        assert_eq!(wrap_text("abcdef", 3), vec!["abc", "def"]);
    }

    #[test]
    fn wrap_text_prefers_word_boundaries() {
        assert_eq!(
            wrap_text("alpha beta gamma", 10),
            vec!["alpha beta", "gamma"]
        );
    }

    // Keep DataState with cats alive for test helpers that still use it
    // (will be cleaned in Step 5).
    #[allow(dead_code)]
    fn _cats_helper() -> DataState {
        DataState {
            cats: vec![CatData {
                cat: Category::Prs,
                items: vec![],
            }],
            ..DataState::default()
        }
    }
}
