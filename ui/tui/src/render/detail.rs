use crate::display::{item_detail_line, item_urgency, DisplayItem, ListSnapshot};
use crate::state::DetailView;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders},
};

pub(super) fn render_detail(
    frame: &mut ratatui::Frame,
    view: &mut DetailView,
    parent: &ListSnapshot,
    content_area: Rect,
) {
    let group_data = parent.items.get(view.group_index).and_then(|d| {
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

    super::render_list_view(
        frame,
        inner,
        items,
        &mut view.list_state,
        |item| (item_detail_line(item).flat(), None, item_urgency(item)),
        |_| None,
    );
}
