use super::{dim, list_highlight, FOCUS_COLOR};
use crate::display::{
    flat_row_line, flat_row_urgency, log_detail_view_from_group, log_detail_view_from_item,
    DisplayItem, Filter, FlatRow, LineParts, RowSeparator,
};
use crate::render::shared::{
    bullet_span, push_segments, segment_chars, urgency_color, urgency_style,
};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph},
};

pub(crate) fn build_unified_list_item(
    text_style: Style,
    bullet_color: Color,
    parts: LineParts,
    inner_width: usize,
) -> ListItem<'static> {
    let chrome_width =
        parts.source.as_ref().map_or(0, |s| s.chars().count() + 3) + parts.age.chars().count();
    let chrome_total = if chrome_width > 0 {
        2 + chrome_width
    } else {
        0
    };
    let separator_width = match parts.separator {
        RowSeparator::TreeChild(_) => 4, // "  │ " / "  └ "
        _ => 3,                          // " · " / " ▸ " / " ▾ "
    };
    let category_prefix_width = parts.category.chars().count() + separator_width;
    let content_budget = inner_width
        .saturating_sub(chrome_total)
        .saturating_sub(category_prefix_width);

    let flat = parts.flat();
    let display_text = crate::display::truncate_to_width(&flat, content_budget);
    let display_chars: Vec<char> = display_text.chars().collect();
    let display_len = display_chars.len();

    let primary_len = segment_chars(&parts.primary);
    let inline_len = segment_chars(&parts.dim_inline);
    let bright_end = display_len.min(primary_len);
    let dim_inline_end = display_len.min(primary_len + inline_len);

    let padding = content_budget.saturating_sub(display_len);

    let separator = match parts.separator {
        RowSeparator::Bullet => bullet_span(bullet_color),
        RowSeparator::Toggle(expanded) => {
            let arrow = if expanded { "▾" } else { "▸" };
            Span::styled(format!(" {} ", arrow), Style::default().fg(bullet_color))
        }
        RowSeparator::TreeChild(last) => {
            let bar = if last { "└" } else { "│" };
            Span::styled(format!("  {} ", bar), Style::default().fg(bullet_color))
        }
    };
    let mut spans: Vec<Span<'static>> = vec![Span::styled(parts.category, text_style), separator];
    push_segments(
        &mut spans,
        &parts.primary,
        &display_chars[..bright_end],
        Style::default(),
        bullet_color,
    );
    push_segments(
        &mut spans,
        &parts.dim_inline,
        &display_chars[bright_end..dim_inline_end],
        dim(),
        bullet_color,
    );
    if padding > 0 {
        spans.push(Span::raw(" ".repeat(padding)));
    }
    if chrome_width > 0 {
        spans.push(Span::raw("  "));
        if let Some(source) = parts.source {
            spans.push(Span::styled(source, dim()));
            spans.push(bullet_span(bullet_color));
        }
        spans.push(Span::styled(parts.age, dim()));
    }
    ListItem::new(Line::from(spans))
}

pub(super) fn wrap_text(text: &str, width: usize) -> Vec<String> {
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

pub(crate) fn unified_title(filter: &Filter, query_input: Option<&str>) -> String {
    if let Some(q) = query_input {
        let cat_prefix = filter
            .category
            .map(|c| format!("{} · ", c.label()))
            .unwrap_or_default();
        return format!(" {cat_prefix}\"{q}\" ");
    }
    match (&filter.category, &filter.query) {
        (None, None) => " All ".to_string(),
        (Some(cat), None) => format!(" {} ", cat.label()),
        (None, Some(q)) => format!(" \"{}\" ", q),
        (Some(cat), Some(q)) => format!(" {} · \"{}\" ", cat.label(), q),
    }
}

/// Returns the style for filter-related chrome (border, dividers).
/// Yellow while a query is being typed; mauve once any filter is committed; dim otherwise.
pub(crate) fn filter_chrome_style(filter: &Filter, query_input: Option<&str>) -> Style {
    if query_input.is_some() {
        Style::default().fg(Color::Yellow)
    } else if !filter.is_empty() {
        Style::default().fg(FOCUS_COLOR)
    } else {
        dim()
    }
}

pub(crate) fn urgency_divider(width: usize, chrome: Style) -> ListItem<'static> {
    let line = "─".repeat(width);
    ListItem::new(Line::from(Span::styled(line, chrome)))
}

pub(crate) fn render_unified(
    frame: &mut ratatui::Frame,
    flat_rows: &[FlatRow],
    selected: usize,
    filter: &Filter,
    query_input: Option<&str>,
    area: Rect,
) {
    let chrome = filter_chrome_style(filter, query_input);
    let title_style = chrome
        .add_modifier(Modifier::BOLD)
        .remove_modifier(Modifier::DIM);
    let title = Span::styled(unified_title(filter, query_input), title_style);
    let block = Block::new()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(chrome);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = inner.width as usize;

    // Build display rows: inject urgency dividers at tier boundaries.
    let mut display_items: Vec<ListItem> = vec![];
    let mut selected_display: Option<usize> = None;
    let mut prev_urgency: Option<domain::Urgency> = None;
    let mut divider_rows: Vec<usize> = vec![];

    for (row_idx, row) in flat_rows.iter().enumerate() {
        let urgency = flat_row_urgency(row);
        if prev_urgency.is_some() && Some(urgency) != prev_urgency {
            divider_rows.push(display_items.len());
            display_items.push(urgency_divider(width, chrome));
        }
        prev_urgency = Some(urgency);

        if row_idx == selected {
            selected_display = Some(display_items.len());
        }

        let parts = flat_row_line(row);
        let text_style = urgency_style(urgency);
        let bullet_color = urgency_color(urgency);

        display_items.push(build_unified_list_item(
            text_style,
            bullet_color,
            parts,
            width,
        ));
    }

    let list = List::new(display_items).highlight_style(list_highlight());
    let mut ls = ListState::default();
    ls.select(selected_display);
    frame.render_stateful_widget(list, inner, &mut ls);

    // Overwrite border cells at divider rows with T-junction characters so
    // the horizontal lines visually connect to the side borders.
    let scroll = ls.offset();
    for row in divider_rows {
        if row < scroll {
            continue;
        }
        let screen_y = inner.y + (row - scroll) as u16;
        if screen_y >= inner.y + inner.height {
            break;
        }
        let buf = frame.buffer_mut();
        buf.set_string(area.x, screen_y, "├", chrome);
        buf.set_string(area.x + area.width - 1, screen_y, "┤", chrome);
    }
}

pub(crate) fn render_split_detail_pane(
    frame: &mut ratatui::Frame,
    area: Rect,
    items: &[DisplayItem],
    selected_row: Option<&FlatRow>,
    detail_scroll: &mut u16,
    stream_blocks: &[domain::StreamBlock],
    border_style: Style,
) {
    match selected_row {
        Some(FlatRow::Single(item)) | Some(FlatRow::GroupChild { item, .. }) => match item {
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

const LIST_BORDER_LINES: u16 = 2;

// Pure arithmetic extracted for property testing.
pub(crate) fn unified_list_height_from_counts(
    item_count: usize,
    divider_count: usize,
    max_height: u16,
) -> u16 {
    let needed = (item_count + divider_count + LIST_BORDER_LINES as usize)
        .try_into()
        .unwrap_or(u16::MAX);
    needed.min(max_height)
}

// Height of the list box in split view: content rows + urgency dividers + borders, capped at
// max_height. Mirrors the divider injection logic in render_unified — keep in sync.
pub(crate) fn unified_list_height(rows: &[FlatRow], max_height: u16) -> u16 {
    let divider_count = rows
        .windows(2)
        .filter(|w| flat_row_urgency(&w[0]) != flat_row_urgency(&w[1]))
        .count();
    unified_list_height_from_counts(rows.len(), divider_count, max_height)
}
