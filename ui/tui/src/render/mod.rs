use chrono::Utc;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, ListItem, Paragraph},
};

use crate::display::DisplayItem;
use crate::state::{
    compute_enter_action, compute_investigate_action, App, EnterAction, InvestigateAction, View,
};

mod category;
mod detail;
mod home;

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

const KEYBINDS_HOME: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("h", "left (or prev tile)"),
    ("j", "down (or next tile)"),
    ("k", "up (or prev tile)"),
    ("l", "right (or next tile)"),
    ("Tab", "next tile"),
    ("Shift-Tab", "prev tile"),
    ("Enter", "drill into category"),
    ("q / Ctrl-C", "quit"),
];

const KEYBINDS_CATEGORY: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("h", "previous"),
    ("j", "down"),
    ("k", "up"),
    ("l", "next"),
    ("Enter", "open / drill into group"),
    ("i", "investigate CI failure"),
    ("Esc", "back to home"),
    ("q / Ctrl-C", "quit"),
];

const KEYBINDS_DETAIL: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("h", "previous"),
    ("j", "down"),
    ("k", "up"),
    ("l", "next"),
    ("Enter", "open URL"),
    ("i", "investigate CI failure"),
    ("Esc", "back to category"),
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

fn status_bar_left(app: &App) -> String {
    if let Some(flash) = &app.flash {
        return flash.clone();
    }
    let enter_action = compute_enter_action(app);
    let investigate_action = compute_investigate_action(app);
    match app.current_view() {
        View::Home => {
            let total: usize = app.cats.iter().map(|c| c.items.len()).sum();
            format!("{total} items")
        }
        View::Category { cat, list_state } => {
            let n = app
                .cats
                .iter()
                .find(|c| c.cat == *cat)
                .map(|c| c.items.len())
                .unwrap_or(0);
            let pos = list_state
                .selected()
                .map(|i| format!("{}/{n}", i + 1))
                .unwrap_or_default();
            let enter_hint = match &enter_action {
                EnterAction::OpenUrl(url) => format!(" · Press ↩ to open {url}"),
                EnterAction::OpenDetail { item_count, .. } => {
                    format!(" · Press ↩ to expand ({item_count} items)")
                }
                _ => String::new(),
            };
            let inv_hint = if matches!(investigate_action, InvestigateAction::LaunchCi { .. }) {
                " · Press i to investigate"
            } else {
                ""
            };
            format!("{pos}{enter_hint}{inv_hint}")
        }
        View::Detail {
            cat,
            group_index,
            list_state,
        } => {
            let cd = app.cats.iter().find(|c| c.cat == *cat);
            let count = match cd.and_then(|c| c.items.get(*group_index)) {
                Some(DisplayItem::Group { items, .. }) => items.len(),
                _ => 0,
            };
            let pos = list_state
                .selected()
                .map(|i| format!("{}/{count}", i + 1))
                .unwrap_or_default();
            let enter_hint = match &enter_action {
                EnterAction::OpenUrl(url) => format!(" · Press ↩ to open {url}"),
                _ => String::new(),
            };
            let inv_hint = if matches!(investigate_action, InvestigateAction::LaunchCi { .. }) {
                " · Press i to investigate"
            } else {
                ""
            };
            format!("{pos}{enter_hint}{inv_hint}")
        }
    }
}

pub(crate) fn render(frame: &mut ratatui::Frame, app: &mut App) {
    let [content_area, bar_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    if matches!(app.current_view(), View::Home) {
        home::render_home(frame, app, content_area);
    } else if matches!(app.current_view(), View::Category { .. }) {
        category::render_category(frame, app, content_area);
    } else if matches!(app.current_view(), View::Detail { .. }) {
        detail::render_detail(frame, app, content_area);
    }

    let right_status = if app.is_refreshing {
        if let Some(err) = &app.error {
            format!("refresh failed: {err}")
        } else {
            "refreshing…".to_string()
        }
    } else if let Some(t) = app.last_updated {
        let mins = (Utc::now() - t).num_minutes();
        if mins == 0 {
            "updated just now".to_string()
        } else {
            format!("updated {mins}m ago")
        }
    } else {
        String::new()
    };

    let left = status_bar_left(app);

    let right_width = right_status.chars().count() as u16;
    let [bar_left, bar_right] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(bar_area);
    frame.render_widget(Paragraph::new(left).style(dim()), bar_left);
    frame.render_widget(Paragraph::new(right_status).style(dim()), bar_right);

    if app.show_help {
        let keybinds = match app.current_view() {
            View::Home => KEYBINDS_HOME,
            View::Category { .. } => KEYBINDS_CATEGORY,
            View::Detail { .. } => KEYBINDS_DETAIL,
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
    use super::{status_bar_left, wrap_text};
    use crate::display::{CatData, Category, DisplayItem};
    use crate::state::{App, View};
    use ratatui::widgets::ListState;
    use workflows::status::StatusItem;

    fn minimal_app() -> App {
        App {
            cats: vec![],
            focused_tile: 0,
            views: vec![View::Home],
            is_refreshing: false,
            last_updated: None,
            error: None,
            show_help: false,
            flash: None,
        }
    }

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

    #[test]
    fn status_bar_shows_flash_when_set() {
        let app = App {
            flash: Some("something went wrong".to_string()),
            ..minimal_app()
        };
        assert_eq!(status_bar_left(&app), "something went wrong");
    }

    #[test]
    fn status_bar_home_shows_total_item_count() {
        let app = App {
            cats: vec![
                CatData {
                    cat: Category::Errors,
                    items: vec![DisplayItem::Single(pr())],
                },
                CatData {
                    cat: Category::Prs,
                    items: vec![DisplayItem::Single(pr()), DisplayItem::Single(pr())],
                },
                CatData {
                    cat: Category::Issues,
                    items: vec![],
                },
            ],
            ..minimal_app()
        };
        assert_eq!(status_bar_left(&app), "3 items");
    }

    #[test]
    fn status_bar_category_shows_position() {
        let mut list_state = ListState::default();
        list_state.select(Some(1));
        let app = App {
            cats: vec![CatData {
                cat: Category::Prs,
                items: vec![DisplayItem::Single(pr()), DisplayItem::Single(pr())],
            }],
            views: vec![View::Category {
                cat: Category::Prs,
                list_state,
            }],
            ..minimal_app()
        };
        assert!(status_bar_left(&app).starts_with("2/2"));
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
}
