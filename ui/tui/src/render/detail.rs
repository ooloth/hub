use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, List},
};
use workflows::status::StatusItem;

use crate::display::{item_line, item_urgency, item_url, DisplayItem};
use crate::state::{App, View};

pub(super) fn render_detail(frame: &mut ratatui::Frame, app: &mut App, content_area: Rect) {
    let View::Detail {
        ref cat,
        ref group_index,
        ref mut list_state,
    } = *app.views.last_mut().unwrap()
    else {
        return;
    };

    let group_data = app
        .cats
        .iter()
        .find(|c| c.cat == *cat)
        .and_then(|c| c.items.get(*group_index))
        .and_then(|d| {
            if let DisplayItem::Group { label, items } = d {
                Some((label.as_str(), items.as_slice()))
            } else {
                None
            }
        });

    let Some((label, items)) = group_data else {
        return;
    };

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

    let list_items: Vec<ratatui::widgets::ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = selected == Some(i);
            let dot_style = if is_selected {
                Style::default()
            } else {
                super::urgency_style(item_urgency(item))
            };
            let hint = if is_selected {
                selected_hint.clone()
            } else {
                None
            };
            let item_width =
                text_width.saturating_sub(hint.as_ref().map_or(0, |h| h.chars().count() + 2));
            super::build_list_item(
                Span::styled("● ", dot_style),
                super::wrap_text(&item_line(item), item_width),
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
