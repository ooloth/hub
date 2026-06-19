//! Detail pane dispatcher: given the selected list row, renders the appropriate
//! signal-specific detail view in the bottom panel of the split-view layout.
//!
//! Each signal type has its own renderer in a sibling module; this module owns
//! only the dispatch logic and the "no detail available" placeholder.

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use crate::display::{log_detail_view_from_group, log_detail_view_from_item, DisplayItem, FlatRow};

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_split_detail_pane(
    frame: &mut ratatui::Frame,
    area: Rect,
    items: &[DisplayItem],
    selected_row: Option<&FlatRow>,
    detail_scroll: &mut u16,
    stream_blocks: &[domain::StreamBlock],
    border_style: Style,
    show_session: bool,
) {
    // When the session toggle is active, render the agent session detail for
    // the attached task of the selected BadgedSignal row.
    if show_session {
        if let Some(task) = selected_row.and_then(FlatRow::attached_task) {
            super::session::render_agent_session_detail(
                frame,
                task,
                stream_blocks,
                detail_scroll,
                area,
                border_style,
            );
            return;
        }
        // No attached task available (shouldn't happen when the guard is
        // correct, but fall through to signal detail rather than panic).
    }

    match selected_row {
        Some(
            FlatRow::Single(item)
            | FlatRow::GroupChild { item, .. }
            | FlatRow::BadgedSignal { item, .. },
        ) => match item {
            workflows::status::StatusItem::Pr(pr) => {
                super::pr::render_pr_detail(frame, pr, detail_scroll, area, border_style);
            }
            workflows::status::StatusItem::Issue(issue) => {
                super::issue::render_issue_detail(frame, issue, detail_scroll, area);
            }
            workflows::status::StatusItem::Gcp(_) | workflows::status::StatusItem::Loki(_) => {
                if let Some(view) = log_detail_view_from_item(item) {
                    super::log::render_log_detail(frame, &view, detail_scroll, area);
                } else {
                    render_detail_placeholder(frame, area);
                }
            }
            workflows::status::StatusItem::AgentSession(task) => {
                super::session::render_agent_session_detail(
                    frame,
                    task,
                    stream_blocks,
                    detail_scroll,
                    area,
                    border_style,
                );
            }
            _ => render_detail_placeholder(frame, area),
        },
        Some(FlatRow::GroupHeader { key, .. }) => {
            let group_items = items.iter().find_map(|di| match di {
                DisplayItem::Group { label, items: gi } if label == key => Some(gi.as_slice()),
                _ => None,
            });
            if let Some(view) = group_items.and_then(log_detail_view_from_group) {
                super::log::render_log_detail(frame, &view, detail_scroll, area);
            } else {
                render_detail_placeholder(frame, area);
            }
        }
        None => render_detail_placeholder(frame, area),
    }
}

pub(crate) fn render_detail_placeholder(frame: &mut ratatui::Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));
    let paragraph = Paragraph::new("No detail view available.")
        .block(block)
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}
