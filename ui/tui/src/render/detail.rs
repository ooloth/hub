use crate::display::{item_hint, item_line, item_urgency, DisplayItem};
use crate::state::{DataState, DetailView};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, List},
};

pub(super) fn render_detail(
    frame: &mut ratatui::Frame,
    view: &mut DetailView,
    data: &DataState,
    content_area: Rect,
) {
    let group_data = data
        .cats
        .iter()
        .find(|c| c.cat == view.cat)
        .and_then(|c| c.items.get(view.group_index))
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
            .fg(super::FOCUS_COLOR)
            .add_modifier(Modifier::BOLD),
    );
    let block = Block::new()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(super::FOCUS_COLOR));
    let inner = block.inner(content_area);
    frame.render_widget(block, content_area);

    let text_width = inner.width.saturating_sub(2) as usize;
    let selected = view.list_state.selected();
    let selected_hint: Option<String> = selected.and_then(|i| items.get(i)).and_then(item_hint);

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

    let list = List::new(list_items).highlight_style(super::list_highlight());
    frame.render_stateful_widget(list, inner, &mut view.list_state);
}
