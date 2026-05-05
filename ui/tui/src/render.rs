use chrono::Utc;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
};
use workflows::status::StatusItem;

use crate::display::{
    display_item_line, display_item_urgency, item_line, item_urgency, item_url, CatData,
    DisplayItem,
};
use crate::state::{
    compute_enter_action, compute_investigate_action, App, EnterAction, InvestigateAction, View,
    TILE_COLS,
};

pub(crate) fn urgency_style(u: domain::Urgency) -> Style {
    match u {
        domain::Urgency::Critical => Style::default().fg(Color::Red),
        domain::Urgency::High => Style::default().fg(Color::Yellow),
        domain::Urgency::Medium => Style::default(),
        domain::Urgency::Low => Style::default().add_modifier(Modifier::DIM),
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
    let dim = Style::default().add_modifier(Modifier::DIM);
    let suffix_span = dim_suffix.map(|s| Span::styled(s, dim));
    let hint_span = hint.map(|h| Span::styled(format!("  {h}"), dim));
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

pub(crate) fn wrap_text(text: &str, width: usize) -> Vec<String> {
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

fn truncate(text: &str, max_width: usize) -> String {
    if text.chars().count() <= max_width {
        text.to_string()
    } else {
        format!(
            "{}…",
            text.chars()
                .take(max_width.saturating_sub(1))
                .collect::<String>()
        )
    }
}

fn render_tile(frame: &mut ratatui::Frame, cat_data: &CatData, focused: bool, area: Rect) {
    let border_style = if focused {
        Style::default().fg(Color::Green)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };

    let count = cat_data.items.len();
    let title_style = if focused {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Reset)
            .remove_modifier(Modifier::DIM)
            .add_modifier(Modifier::BOLD)
    };
    let title = Span::styled(format!(" {} ", cat_data.cat.label()), title_style);
    let block = Block::new()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if count == 0 {
        frame.render_widget(
            Paragraph::new("(none)").style(Style::default().add_modifier(Modifier::DIM)),
            inner,
        );
        return;
    }

    let available = inner.height as usize;
    let has_more = count > available;
    let preview_count = if has_more {
        available.saturating_sub(1)
    } else {
        count.min(available)
    };
    let text_width = (inner.width as usize).saturating_sub(2); // "● "

    let dim = Style::default().add_modifier(Modifier::DIM);
    let mut lines: Vec<Line> = cat_data
        .items
        .iter()
        .take(preview_count)
        .map(|item| {
            let dot_style = urgency_style(display_item_urgency(item));
            let (label, count_suffix) = match item {
                DisplayItem::Group { label, items } => {
                    let suffix = format!(" ({})", items.len());
                    let label_width = text_width.saturating_sub(suffix.chars().count());
                    (truncate(label, label_width), Some(suffix))
                }
                DisplayItem::Single(_) => (truncate(&display_item_line(item), text_width), None),
            };
            let mut spans = vec![Span::styled("● ", dot_style), Span::raw(label)];
            if let Some(s) = count_suffix {
                spans.push(Span::styled(s, dim));
            }
            Line::from(spans)
        })
        .collect();

    if has_more {
        let more = count - preview_count;
        lines.push(Line::from(Span::styled(
            format!("  ↓ {more} more"),
            Style::default().add_modifier(Modifier::DIM),
        )));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn render_home(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let n = app.cats.len();
    if n == 0 {
        return;
    }
    let cols = TILE_COLS;
    let rows = n.div_ceil(cols);

    let row_areas = Layout::vertical(
        (0..rows)
            .map(|_| Constraint::Ratio(1, rows as u32))
            .collect::<Vec<_>>(),
    )
    .split(area);

    for (row_idx, &row_area) in row_areas.iter().enumerate() {
        let start = row_idx * cols;
        let end = (start + cols).min(n);
        let count = end - start;
        let col_areas = Layout::horizontal(
            (0..count)
                .map(|_| Constraint::Ratio(1, count as u32))
                .collect::<Vec<_>>(),
        )
        .split(row_area);
        for (col_idx, &col_area) in col_areas.iter().enumerate() {
            let tile_idx = start + col_idx;
            render_tile(
                frame,
                &app.cats[tile_idx],
                tile_idx == app.focused_tile,
                col_area,
            );
        }
    }
}

pub(crate) fn render(frame: &mut ratatui::Frame, app: &mut App) {
    let [content_area, bar_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    if matches!(app.view, View::Home) {
        render_home(frame, app, content_area);
    } else if let View::Category {
        cat,
        ref mut list_state,
    } = app.view
    {
        let cat_items = app
            .cats
            .iter()
            .find(|c| c.cat == cat)
            .map(|c| c.items.as_slice())
            .unwrap_or(&[]);
        let title = Span::styled(
            format!(" {} ", cat.label()),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        );
        let block = Block::new()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Green));
        let inner = block.inner(content_area);
        frame.render_widget(block, content_area);
        let text_width = inner.width.saturating_sub(2) as usize;
        let selected = list_state.selected();
        let selected_hint: Option<String> =
            selected
                .and_then(|i| cat_items.get(i))
                .and_then(|item| match item {
                    DisplayItem::Group { .. } => Some("↩ to expand".to_string()),
                    DisplayItem::Single(StatusItem::Ci(_)) => {
                        Some("↩ to open · i to investigate".to_string())
                    }
                    DisplayItem::Single(s) => item_url(s).map(|_| "↩ to open".to_string()),
                });
        let list_items: Vec<ListItem> = cat_items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let is_selected = selected == Some(i);
                let dot_style = if is_selected {
                    Style::default()
                } else {
                    urgency_style(display_item_urgency(item))
                };
                let hint = if is_selected {
                    selected_hint.clone()
                } else {
                    None
                };
                let (line_text, dim_suffix) = match item {
                    DisplayItem::Group { label, items } => (
                        label.as_str().to_string(),
                        Some(format!(" ({})", items.len())),
                    ),
                    DisplayItem::Single(_) => (display_item_line(item), None),
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
        let list = List::new(list_items).highlight_style(
            Style::default()
                .bg(Color::Rgb(41, 45, 62))
                .add_modifier(Modifier::BOLD),
        );
        frame.render_stateful_widget(list, inner, list_state);
    } else if let View::Detail {
        cat,
        group_index,
        ref mut list_state,
    } = app.view
    {
        let group_data = app
            .cats
            .iter()
            .find(|c| c.cat == cat)
            .and_then(|c| c.items.get(group_index))
            .and_then(|d| {
                if let DisplayItem::Group { label, items } = d {
                    Some((label.as_str(), items.as_slice()))
                } else {
                    None
                }
            });
        if let Some((label, items)) = group_data {
            let title = Span::styled(
                format!(" {} ", label),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            );
            let block = Block::new()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Green));
            let inner = block.inner(content_area);
            frame.render_widget(block, content_area);
            let text_width = inner.width.saturating_sub(2) as usize;
            let selected = list_state.selected();
            let selected_hint: Option<String> =
                selected
                    .and_then(|i| items.get(i))
                    .and_then(|item| match item {
                        StatusItem::Ci(_) => Some("↩ to open · i to investigate".to_string()),
                        item => item_url(item).map(|_| "↩ to open".to_string()),
                    });
            let list_items: Vec<ListItem> = items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let is_selected = selected == Some(i);
                    let dot_style = if is_selected {
                        Style::default()
                    } else {
                        urgency_style(item_urgency(item))
                    };
                    let hint = if is_selected {
                        selected_hint.clone()
                    } else {
                        None
                    };
                    let item_width = text_width
                        .saturating_sub(hint.as_ref().map_or(0, |h| h.chars().count() + 2));
                    build_list_item(
                        Span::styled("● ", dot_style),
                        wrap_text(&item_line(item), item_width),
                        None,
                        hint,
                    )
                })
                .collect();
            let list = List::new(list_items).highlight_style(
                Style::default()
                    .bg(Color::Rgb(41, 45, 62))
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_stateful_widget(list, inner, list_state);
        }
    }

    // Status bar
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

    let enter_action = compute_enter_action(app);
    let investigate_action = compute_investigate_action(app);

    let left = if let Some(flash) = &app.flash {
        flash.clone()
    } else {
        match &app.view {
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
    };

    let right_width = right_status.chars().count() as u16;
    let [bar_left, bar_right] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(bar_area);
    let dim = Style::default().add_modifier(Modifier::DIM);
    frame.render_widget(Paragraph::new(left).style(dim), bar_left);
    frame.render_widget(Paragraph::new(right_status).style(dim), bar_right);

    if app.show_help {
        let keybinds = match &app.view {
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
    use super::wrap_text;

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
