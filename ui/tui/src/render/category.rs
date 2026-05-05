use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, List},
};
use workflows::status::StatusItem;

use crate::display::{display_item_line, display_item_urgency, item_url, DisplayItem};
use crate::state::{App, View};

pub(super) fn render_category(frame: &mut ratatui::Frame, app: &mut App, content_area: Rect) {
    let View::Category {
        ref cat,
        ref mut list_state,
    } = *app.views.last_mut().unwrap()
    else {
        return;
    };

    let cat_items = app
        .cats
        .iter()
        .find(|c| c.cat == *cat)
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

    let list_items: Vec<ratatui::widgets::ListItem> = cat_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = selected == Some(i);
            let dot_style = if is_selected {
                Style::default()
            } else {
                super::urgency_style(display_item_urgency(item))
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
            super::build_list_item(
                Span::styled("● ", dot_style),
                super::wrap_text(&line_text, item_width),
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
}
