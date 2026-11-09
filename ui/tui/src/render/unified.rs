use super::{dim, list_highlight, FOCUS_COLOR};
use crate::display::{flat_row_line, flat_row_urgency, Filter, FlatRow, LineParts, RowSeparator};
use crate::render::shared::{
    bullet_span, push_segments, segment_chars, urgency_color, urgency_style,
};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState},
};

#[allow(clippy::indexing_slicing)] // bright_end/dim_inline_end are bounded by display_len == display_chars.len()
pub(crate) fn build_unified_list_item(
    text_style: Style,
    bullet_color: Color,
    parts: LineParts,
    inner_width: usize,
) -> ListItem<'static> {
    let source_width = match &parts.source {
        Some(source) => source.chars().count() + 3,
        None => 0,
    };
    let chrome_width = source_width + parts.age.chars().count();
    let chrome_total = if chrome_width > 0 {
        2 + chrome_width
    } else {
        0
    };
    let separator_width = match parts.separator {
        RowSeparator::TreeChild(_) => 4, // "  │ " / "  └ "
        _ => 3,                          // " · " / " ▸ " / " ▾ "
    };
    // A subcategory narrows the category, so it shares the label's width, which is never truncated.
    let subcategory_width = match &parts.subcategory {
        Some(subcategory) => 3 + subcategory.chars().count(),
        None => 0,
    };
    let label_width = parts.category.chars().count() + subcategory_width;
    let category_prefix_width = label_width + separator_width;
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
            Span::styled(format!(" {arrow} "), Style::default().fg(bullet_color))
        }
        RowSeparator::TreeChild(last) => {
            let bar = if last { "└" } else { "│" };
            Span::styled(format!("  {bar} "), Style::default().fg(bullet_color))
        }
    };
    let mut spans: Vec<Span<'static>> = vec![Span::styled(parts.category, text_style)];
    if let Some(subcategory) = parts.subcategory {
        spans.push(bullet_span(bullet_color));
        spans.push(Span::styled(subcategory, text_style));
    }
    spans.push(separator);
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

#[allow(clippy::indexing_slicing)] // bounds maintained by while-loop guards and min(total)
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
        let cat_prefix = match filter.category {
            Some(c) => format!("{} · ", c.label()),
            None => String::new(),
        };
        return format!(" {cat_prefix}\"{q}\" ");
    }
    match (&filter.category, &filter.query) {
        (None, None) => " All ".to_string(),
        (Some(cat), None) => format!(" {} ", cat.label()),
        (None, Some(q)) => format!(" \"{q}\" "),
        (Some(cat), Some(q)) => format!(" {} · \"{}\" ", cat.label(), q),
    }
}

/// The list's position for the top-right of its border: the selected row's number out of every
/// row in the filtered list, or nothing for an empty list.
///
/// Both numbers count `flat_rows`, so rows scrolled out of view count, urgency dividers do not,
/// and an expanded group's children count as rows, so the total grows when a group is expanded.
pub(crate) fn list_position_title(selected: usize, row_count: usize) -> Option<String> {
    if row_count == 0 {
        return None;
    }
    assert!(
        selected < row_count,
        "selected row {selected} is past the last of {row_count} rows"
    );
    Some(format!(" {}/{row_count} ", selected + 1))
}

/// Whether the position fits on the top border beside the filter title, with a column between
/// them. ratatui draws right-aligned titles after left ones, so a position that does not fit
/// would be printed over the end of the filter title.
pub(crate) fn position_title_fits(left: &Line, right: &Line, block_width: u16) -> bool {
    let corners = 2;
    let gap = 1;
    left.width() + gap + right.width() + corners <= usize::from(block_width)
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

/// The list border's title style: the border's colour, bold and never dim.
/// The status bar draws its keys in it too, so the two always match.
pub(crate) fn title_style(filter: &Filter, query_input: Option<&str>) -> Style {
    filter_chrome_style(filter, query_input)
        .add_modifier(Modifier::BOLD)
        .remove_modifier(Modifier::DIM)
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
    let title_style = title_style(filter, query_input);
    let title = Line::from(Span::styled(
        unified_title(filter, query_input),
        title_style,
    ));
    let position = list_position_title(selected, flat_rows.len())
        .map(|text| Line::from(Span::styled(text, title_style)).right_aligned())
        .filter(|position| position_title_fits(&title, position, area.width));
    let mut block = Block::new()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(chrome);
    if let Some(position) = position {
        block = block.title(position);
    }
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
        let screen_y =
            inner.y + u16::try_from((row - scroll).min(usize::from(u16::MAX))).unwrap_or(u16::MAX);
        if screen_y >= inner.y + inner.height {
            break;
        }
        let buf = frame.buffer_mut();
        buf.set_string(area.x, screen_y, "├", chrome);
        buf.set_string(area.x + area.width - 1, screen_y, "┤", chrome);
    }
}

#[cfg(test)]
mod tests {
    use super::{list_position_title, position_title_fits, render_unified};
    use crate::display::{Filter, FlatRow};
    use crate::test_support::pull_request;
    use proptest::{prelude::Strategy, proptest};
    use ratatui::{backend::TestBackend, buffer::Buffer, text::Line, Terminal};
    use rstest::rstest;
    use workflows::status::StatusItem;

    fn pr() -> StatusItem {
        StatusItem::Pr(pull_request())
    }

    /// The list block drawn `width` columns wide, with `query_input` as the query being typed.
    fn draw_list(
        rows: &[FlatRow],
        filter: &Filter,
        query_input: Option<&str>,
        width: u16,
    ) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, 4)).unwrap();
        let _ = terminal
            .draw(|frame| render_unified(frame, rows, 0, filter, query_input, frame.area()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The top border of the list block drawn `width` columns wide.
    fn top_border(rows: &[FlatRow], filter: &Filter, width: u16) -> String {
        let buf = draw_list(rows, filter, None, width);
        (0..width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect()
    }

    #[test]
    fn an_empty_list_has_no_position() {
        assert_eq!(list_position_title(0, 0), None);
    }

    #[test]
    fn the_last_row_shows_the_total() {
        assert_eq!(list_position_title(2, 3).as_deref(), Some(" 3/3 "));
    }

    #[rstest]
    #[case::no_filter(Filter::default(), None)]
    #[case::committed_filter(Filter { query: Some("hub".to_string()), ..Filter::default() }, None)]
    #[case::typing_a_query(Filter::default(), Some("hub"))]
    fn the_position_has_the_filter_titles_style(
        #[case] filter: Filter,
        #[case] query_input: Option<&str>,
    ) {
        let rows = vec![FlatRow::Single(pr())];
        let width = 40;
        let buf = draw_list(&rows, &filter, query_input, width);
        // The title's first letter follows the corner and a space. The count ` 1/1 ` ends a space
        // and a corner from the right edge, so its last digit is three columns in.
        let title_cell = &buf[(2, 0)];
        let position_cell = &buf[(width - 3, 0)];
        assert_eq!(position_cell.symbol(), "1");
        assert_eq!(position_cell.style(), title_cell.style());
    }

    #[test]
    #[should_panic(expected = "selected row 3 is past the last of 3 rows")]
    fn a_selection_past_the_last_row_halts() {
        let _ = list_position_title(3, 3);
    }

    #[test]
    fn the_position_shows_at_the_narrowest_width_that_fits_it() {
        // " All " (5) + a gap (1) + " 1/1 " (5) + two corners (2) = 13.
        let rows = vec![FlatRow::Single(pr())];
        assert!(top_border(&rows, &Filter::default(), 13).contains(" 1/1 "));
    }

    #[test]
    fn the_position_is_left_out_one_column_narrower() {
        let rows = vec![FlatRow::Single(pr())];
        let border = top_border(&rows, &Filter::default(), 12);
        assert!(border.contains(" All "), "{border}");
        assert!(!border.contains('/'), "{border}");
    }

    proptest! {
        #[test]
        fn the_position_is_a_row_number_out_of_the_row_count(
            (row_count, selected) in (1usize..=100).prop_flat_map(|n| (proptest::strategy::Just(n), 0..n)),
        ) {
            let title = list_position_title(selected, row_count).expect("a non-empty list has a position");
            let (number, total) = title.trim().split_once('/').expect("a position has a slash");
            let number: usize = number.parse().unwrap();
            let total: usize = total.parse().unwrap();
            assert_eq!(total, row_count);
            assert!((1..=total).contains(&number));
            assert_eq!(number == total, selected == row_count - 1);
        }

        #[test]
        fn the_position_never_overwrites_the_filter_title(
            width in 4u16..=120,
            query in "[a-z]{0,60}",
            row_count in 1usize..=12,
        ) {
            let rows: Vec<FlatRow> = (0..row_count).map(|_| FlatRow::Single(pr())).collect();
            let filter = Filter { query: Some(query.clone()), ..Filter::default() };
            let left = format!(" \"{query}\" ");
            let right = list_position_title(0, row_count).expect("a non-empty list has a position");
            let border = top_border(&rows, &filter, width);
            let fits = position_title_fits(&Line::from(left.as_str()), &Line::from(right.as_str()), width);
            if usize::from(width) >= left.chars().count() + 2 {
                assert!(border.contains(&left), "{border}");
            }
            assert_eq!(border.contains(&right), fits, "{border}");
            assert_eq!(fits, left.chars().count() + 1 + right.chars().count() + 2 <= usize::from(width));
        }
    }
}
