use chrono::Utc;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::display::{
    display_item_line, display_item_urgency, format_age_short, DisplayItem, Filter, LineParts,
};
use crate::state::{
    compute_enter_action, compute_investigate_action, App, EnterAction, InvestigateAction,
    RefreshState, Screen,
};

mod detail;

pub(super) const FOCUS_COLOR: Color = Color::Rgb(203, 166, 247); // Catppuccin Mocha Mauve
pub(super) const LAVENDER: Color = Color::Rgb(180, 190, 254); // Catppuccin Mocha Lavender
pub(super) const SELECTION_BG: Color = Color::Rgb(41, 45, 62);

pub(super) fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub(super) fn list_highlight() -> Style {
    Style::default()
        .bg(SELECTION_BG)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn urgency_color(u: domain::Urgency) -> Color {
    match u {
        domain::Urgency::Critical => Color::Red,
        domain::Urgency::High => Color::Yellow,
        domain::Urgency::Medium => Color::Cyan,
        domain::Urgency::Low => Color::Blue,
    }
}

pub(crate) fn urgency_style(u: domain::Urgency) -> Style {
    Style::default().fg(urgency_color(u))
}

fn bullet_span(color: Color) -> Span<'static> {
    Span::styled(" · ", Style::default().fg(color))
}

fn segment_chars(segments: &[String]) -> usize {
    segments.iter().map(|s| s.chars().count()).sum::<usize>() + segments.len().saturating_sub(1) * 3
}

fn push_segments(
    spans: &mut Vec<Span<'static>>,
    segments: &[String],
    chars: &[char],
    style: Style,
    bullet_color: Color,
) {
    if chars.is_empty() {
        return;
    }
    let mut pos = 0;
    for (i, seg) in segments.iter().enumerate() {
        if pos >= chars.len() {
            break;
        }
        if i > 0 {
            let sep_end = (pos + 3).min(chars.len());
            if sep_end - pos == 3 {
                spans.push(bullet_span(bullet_color));
            } else {
                let partial: String = chars[pos..sep_end].iter().collect();
                if !partial.is_empty() {
                    spans.push(Span::styled(partial, style));
                }
            }
            pos = sep_end;
        }
        if pos >= chars.len() {
            break;
        }
        let take = seg.chars().count().min(chars.len() - pos);
        if take > 0 {
            let text: String = chars[pos..pos + take].iter().collect();
            spans.push(Span::styled(text, style));
            pos += take;
        }
    }
}

const KEYBINDS_LIST: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("h / k", "up"),
    ("j / l", "down"),
    ("gg / G", "go to top / bottom"),
    ("Ctrl-u / Ctrl-d", "page up / down"),
    ("Enter", "open / drill into group"),
    ("i", "investigate"),
    ("p / e / o", "filter PRs / Errors / Issues"),
    ("/", "search"),
    ("a / Esc", "clear filter"),
    ("r", "refresh"),
    ("q / Ctrl-C", "quit"),
];

const KEYBINDS_DETAIL: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("h / k", "up"),
    ("j / l", "down"),
    ("gg / G", "go to top / bottom"),
    ("Ctrl-u / Ctrl-d", "page up / down"),
    ("Enter", "open URL"),
    ("i", "investigate"),
    ("r", "refresh"),
    ("Esc", "back to list"),
    ("q / Ctrl-C", "quit"),
];

const KEYBINDS_PR_READER: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("k / j", "scroll up / down"),
    ("gg / G", "go to top / bottom"),
    ("Ctrl-u / Ctrl-d", "page up / down"),
    ("Enter", "open in browser"),
    ("i", "investigate"),
    ("r", "refresh"),
    ("Esc", "back to list"),
    ("q / Ctrl-C", "quit"),
];

const KEYBINDS_ISSUE_READER: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("k / j", "scroll up / down"),
    ("gg / G", "go to top / bottom"),
    ("Ctrl-u / Ctrl-d", "page up / down"),
    ("a", "approve for agent"),
    ("d", "dismiss as won't fix"),
    ("Enter", "open in browser"),
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
    item_data: impl Fn(&T) -> (Option<String>, Vec<String>, Option<String>, domain::Urgency),
    hint_fn: impl Fn(&T) -> Option<String>,
) {
    let full_width = area.width as usize;
    let selected = list_state.selected();
    let selected_hint = selected.and_then(|i| items.get(i)).and_then(hint_fn);
    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = selected == Some(i);
            let (label, primary_segs, dim_suffix, urgency) = item_data(item);
            let bullet_color = urgency_color(urgency);
            let hint = if is_selected {
                selected_hint.clone()
            } else {
                None
            };
            let labels: Vec<Span<'static>> = match label {
                Some(l) => vec![
                    Span::styled(l, urgency_style(urgency)),
                    bullet_span(bullet_color),
                ],
                None => vec![],
            };
            let label_width: usize = labels.iter().map(|s| s.content.chars().count()).sum();
            let flat_primary = primary_segs.join(" · ");
            let item_width = full_width
                .saturating_sub(label_width)
                .saturating_sub(dim_suffix.as_ref().map_or(0, |s| s.chars().count()))
                .saturating_sub(hint.as_ref().map_or(0, |h| h.chars().count() + 2));
            build_list_item(
                labels,
                primary_segs,
                wrap_text(&flat_primary, item_width),
                dim_suffix,
                hint,
                bullet_color,
            )
        })
        .collect();
    let list = List::new(list_items).highlight_style(list_highlight());
    frame.render_stateful_widget(list, area, list_state);
}

fn build_list_item(
    labels: Vec<Span<'static>>,
    primary_segs: Vec<String>,
    wrapped: Vec<String>,
    dim_suffix: Option<String>,
    hint: Option<String>,
    bullet_color: Color,
) -> ListItem<'static> {
    let indent: usize = labels.iter().map(|s| s.content.chars().count()).sum();
    let suffix_span = dim_suffix.map(|s| Span::styled(s, dim()));
    let hint_span = hint.map(|h| Span::styled(format!("  {h}"), dim()));
    let mut lines: Vec<Line> = wrapped
        .into_iter()
        .enumerate()
        .map(|(j, chunk)| {
            if j == 0 {
                let mut spans = labels.clone();
                let chunk_chars: Vec<char> = chunk.chars().collect();
                push_segments(
                    &mut spans,
                    &primary_segs,
                    &chunk_chars,
                    Style::default(),
                    bullet_color,
                );
                Line::from(spans)
            } else {
                Line::from(Span::raw(format!("{}{chunk}", " ".repeat(indent))))
            }
        })
        .collect();
    if lines.is_empty() {
        lines.push(if labels.is_empty() {
            Line::from(Span::raw(""))
        } else {
            let mut spans = labels;
            spans.push(Span::raw(""));
            Line::from(spans)
        });
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

fn build_unified_list_item(
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
    let category_prefix_width = parts.category.chars().count() + 3; // category + " · "
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

    let mut spans: Vec<Span<'static>> = vec![
        Span::styled(parts.category, text_style),
        bullet_span(bullet_color),
    ];
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
        Screen::IssueDetail { .. } | Screen::DismissingIssue { .. } | Screen::PrDetail { .. } => {
            String::new()
        }
    }
}

fn action_hints(enter: &EnterAction, investigate: &InvestigateAction) -> String {
    let enter_hint = match enter {
        EnterAction::OpenUrl(_) => " · [↩] open".to_string(),
        EnterAction::OpenDetail { item_count, .. } => {
            format!(" · [↩] expand {item_count} items")
        }
        EnterAction::OpenIssueDetail(_) | EnterAction::OpenPrDetail(_) => " · [↩] read".to_string(),
        EnterAction::None => String::new(),
    };
    let inv_hint = if matches!(investigate, InvestigateAction::None) {
        ""
    } else {
        " · [i] investigate"
    };
    format!("{enter_hint}{inv_hint}")
}

/// Returns the number of wrapped lines the body text produces at the given width.
/// Used to clamp scroll in the issue reader.
pub(crate) fn issue_body_line_count(body: Option<&str>, width: usize) -> usize {
    let text = body.unwrap_or("");
    if text.is_empty() {
        return 1; // placeholder "(no description)" is one line
    }
    text.lines()
        .map(|line| {
            if line.is_empty() {
                1
            } else {
                wrap_text(line, width).len()
            }
        })
        .sum()
}

fn status_bar_left(app: &App) -> String {
    if let Some(flash) = &app.ui.flash {
        return flash.clone();
    }
    if matches!(app.ui.screen, Screen::PrDetail { .. }) {
        return " [↩] open · [i] investigate · [Esc] back".to_string();
    }
    if matches!(app.ui.screen, Screen::IssueDetail { .. }) {
        return " [a] approve · [d] dismiss · [↩] open · [Esc] back".to_string();
    }
    if matches!(app.ui.screen, Screen::DismissingIssue { .. }) {
        return " [↩] confirm · [Esc] cancel".to_string();
    }
    let enter_action = compute_enter_action(app);
    let investigate_action = compute_investigate_action(app);
    let pos = position_label(app.current_screen());
    let hints = action_hints(&enter_action, &investigate_action);
    format!("{pos}{hints}")
}

fn render_issue_detail(
    frame: &mut ratatui::Frame,
    issue: &domain::Issue,
    scroll: &mut u16,
    area: Rect,
) {
    let inner_width = area.width.saturating_sub(2) as usize; // subtract block borders

    let raw_body = issue
        .body
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("(no description)");

    // Clamp scroll to actual content height.
    let total_lines = issue_body_line_count(issue.body.as_deref(), inner_width) + 2;
    let viewport_height = area.height.saturating_sub(2) as usize; // subtract block borders
    let max_scroll = total_lines.saturating_sub(viewport_height) as u16;
    *scroll = (*scroll).min(max_scroll);

    let bold = Style::default().add_modifier(Modifier::BOLD);

    // Top title: title (left) + repo · #number (right)
    let left_title = Line::from(format!(" {} ", issue.title)).style(bold);
    let right_title = Line::from(format!(" {} · #{} ", issue.repo, issue.number))
        .style(bold)
        .right_aligned();

    // Bottom-left title: labels
    let bottom_left = if issue.labels.is_empty() {
        None
    } else {
        let mut s = String::from(" ");
        s.push_str(&issue.labels.join(" · "));
        s.push(' ');
        Some(Line::from(s).style(bold))
    };

    // Bottom-right title: author + age
    let bottom_right = Line::from(format!(
        " @{} · {} ",
        issue.author,
        format_age_short(issue.age),
    ))
    .style(bold)
    .right_aligned();

    let mut block = Block::default()
        .title(left_title)
        .title(right_title)
        .title_bottom(bottom_right)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(FOCUS_COLOR));
    if let Some(bt) = bottom_left {
        block = block.title_bottom(bt);
    }

    let mut body = crate::markdown::from_str(raw_body);
    body.lines.insert(0, ratatui::text::Line::from(""));
    body.lines.push(ratatui::text::Line::from(""));

    let paragraph = Paragraph::new(body)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .scroll((*scroll, 0));

    frame.render_widget(paragraph, area);
}

fn parse_hunk_new_start(hunk_header: &str) -> Option<u32> {
    hunk_header
        .split_whitespace()
        .find(|t| t.starts_with('+'))
        .and_then(|t| t.trim_start_matches('+').split(',').next())
        .and_then(|n| n.parse().ok())
}

fn render_thread_comments(out: &mut Vec<Line<'static>>, thread: &domain::ReviewThread) {
    let dim_lav = Style::default()
        .fg(LAVENDER)
        .add_modifier(Modifier::DIM)
        .add_modifier(Modifier::ITALIC);
    let dim = Style::default().add_modifier(Modifier::DIM);
    for comment in &thread.comments {
        let age_str = crate::display::format_age_short(comment.age);
        out.push(Line::styled(
            format!("  @{} · {}", comment.author, age_str),
            dim_lav,
        ));
        for body_line in comment.body.lines() {
            out.push(Line::styled(format!("  {body_line}"), dim));
        }
        out.push(Line::from(""));
    }
}

fn pr_diff_lines(pr: &domain::PullRequest, sep_width: usize) -> Vec<Line<'static>> {
    if pr.changed_files.is_empty() {
        return vec![];
    }

    let sep = Style::default().add_modifier(Modifier::DIM);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let lav = Style::default().fg(LAVENDER);
    let mut out: Vec<Line<'static>> = vec![];

    for file in &pr.changed_files {
        let additions = file.additions;
        let deletions = file.deletions;
        let path = file.path.clone();

        let left = format!(" {path} ");
        let fill_len = sep_width
            .saturating_sub(
                left.chars().count() + format!(" +{additions} -{deletions} ").chars().count(),
            )
            .saturating_sub(4);
        let fill = "─".repeat(fill_len);
        let header = Line::from(vec![
            Span::styled(format!("──{left}"), lav),
            Span::styled(fill, lav),
            Span::styled("──".to_string(), lav),
            Span::styled(format!(" +{additions}"), Style::default().fg(Color::Green)),
            Span::styled(format!(" -{deletions} "), Style::default().fg(Color::Red)),
        ])
        .style(bold);
        out.push(header);

        let file_threads: Vec<&domain::ReviewThread> = pr
            .review_threads
            .iter()
            .filter(|t| t.path == file.path)
            .collect();

        match &file.patch {
            None => {
                out.push(Line::styled(" (binary) ".to_string(), sep));
                for thread in file_threads.iter().filter(|t| t.line.is_none()) {
                    render_thread_comments(&mut out, thread);
                }
            }
            Some(patch) => {
                let mut new_line: u32 = 0;
                for raw in patch.lines() {
                    let line = raw.to_string();
                    if line.starts_with("@@") {
                        out.push(Line::from(""));
                        out.push(Line::styled(line.clone(), Style::default().fg(Color::Cyan)));
                        new_line = parse_hunk_new_start(&line).unwrap_or(0);
                    } else if line.starts_with('+') {
                        out.push(Line::styled(line, Style::default().fg(Color::Green)));
                        for thread in file_threads.iter().filter(|t| t.line == Some(new_line)) {
                            render_thread_comments(&mut out, thread);
                        }
                        new_line += 1;
                    } else if line.starts_with('-') {
                        out.push(Line::styled(line, Style::default().fg(Color::Red)));
                    } else {
                        out.push(Line::from(line));
                        for thread in file_threads.iter().filter(|t| t.line == Some(new_line)) {
                            render_thread_comments(&mut out, thread);
                        }
                        new_line += 1;
                    }
                }
                for thread in file_threads.iter().filter(|t| t.line.is_none()) {
                    render_thread_comments(&mut out, thread);
                }
            }
        }
        out.push(Line::from(""));
    }

    let shown = pr.changed_files.len() as u32;
    let total = pr.total_changed_files;
    if total > shown {
        let hidden = total - shown;
        out.push(Line::styled(
            format!(" … and {hidden} more files not shown "),
            sep,
        ));
    }

    out.push(Line::from(""));
    out
}

fn render_pr_detail(
    frame: &mut ratatui::Frame,
    pr: &domain::PullRequest,
    scroll: &mut u16,
    area: Rect,
) {
    let inner_width = area.width.saturating_sub(2) as usize;

    let bold = Style::default().add_modifier(Modifier::BOLD);

    let left_title = Line::from(format!(" {} ", pr.title)).style(bold);
    let right_title = Line::from(format!(" {} · #{} ", pr.repo, pr.number))
        .style(bold)
        .right_aligned();

    let bottom_right = Line::from(format!(" @{} · {} ", pr.author, format_age_short(pr.age),))
        .style(bold)
        .right_aligned();

    let ci_span = match pr.ci_status {
        Some(domain::CiStatus::Success) => {
            Some(Span::styled("✓", Style::default().fg(Color::Green)))
        }
        Some(domain::CiStatus::Failure) => Some(Span::styled("✗", Style::default().fg(Color::Red))),
        Some(domain::CiStatus::Pending) => {
            Some(Span::styled("…", Style::default().fg(Color::Yellow)))
        }
        Some(domain::CiStatus::Neutral) => Some(Span::styled("~", dim())),
        None => None,
    };
    let review_text = match pr.review_count {
        0 => None,
        1 => Some("1 review".to_string()),
        n => Some(format!("{n} reviews")),
    };
    let bottom_left: Option<Line> = match (ci_span, review_text) {
        (None, None) => None,
        (Some(ci), None) => Some(Line::from(vec![Span::raw(" "), ci, Span::raw(" ")])),
        (None, Some(reviews)) => Some(Line::from(format!(" {reviews} ")).style(bold)),
        (Some(ci), Some(reviews)) => Some(Line::from(vec![
            Span::styled(format!(" {reviews} · "), bold),
            ci,
            Span::raw(" "),
        ])),
    };

    let mut block = Block::default()
        .title(left_title)
        .title(right_title)
        .title_bottom(bottom_right)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(FOCUS_COLOR));
    if let Some(bl) = bottom_left {
        block = block.title_bottom(bl);
    }

    let raw_body = pr
        .body
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("(no description)");

    let mut content = crate::markdown::from_str(raw_body);
    content.lines.insert(0, Line::from(""));
    content.lines.push(Line::from(""));

    let mut diff = pr_diff_lines(pr, inner_width);
    content.lines.append(&mut diff);

    let total_lines = content.lines.len();
    let viewport_height = area.height.saturating_sub(2) as usize;
    let max_scroll = total_lines.saturating_sub(viewport_height) as u16;
    *scroll = (*scroll).min(max_scroll);

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .scroll((*scroll, 0));

    frame.render_widget(paragraph, area);
}

fn unified_title(filter: &Filter, query_input: Option<&str>) -> String {
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
fn filter_chrome_style(filter: &Filter, query_input: Option<&str>) -> Style {
    if query_input.is_some() {
        Style::default().fg(Color::Yellow)
    } else if !filter.is_empty() {
        Style::default().fg(FOCUS_COLOR)
    } else {
        dim()
    }
}

fn urgency_divider(width: usize, chrome: Style) -> ListItem<'static> {
    let line = "─".repeat(width);
    ListItem::new(Line::from(Span::styled(line, chrome)))
}

fn render_unified(
    frame: &mut ratatui::Frame,
    items: &[DisplayItem],
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
    // Track which display-row index corresponds to each item index.
    let mut display_items: Vec<ListItem> = vec![];
    let mut selected_display: Option<usize> = None;
    let mut prev_urgency: Option<domain::Urgency> = None;
    let mut divider_rows: Vec<usize> = vec![];

    for (item_idx, item) in items.iter().enumerate() {
        let urgency = display_item_urgency(item);
        // Inject a divider between urgency tiers, but not before the first group.
        if prev_urgency.is_some() && Some(urgency) != prev_urgency {
            divider_rows.push(display_items.len());
            display_items.push(urgency_divider(width, chrome));
        }
        prev_urgency = Some(urgency);

        if item_idx == selected {
            selected_display = Some(display_items.len());
        }

        let parts = display_item_line(item);

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

fn right_status_text(
    state: &RefreshState,
    last_updated: Option<chrono::DateTime<Utc>>,
    now: chrono::DateTime<Utc>,
) -> String {
    let age_str = |t: chrono::DateTime<Utc>| {
        let mins = (now - t).num_minutes();
        if mins == 0 {
            "just now".to_string()
        } else {
            format!("{mins}m ago")
        }
    };
    match state {
        RefreshState::InProgress => "refreshing…".to_string(),
        RefreshState::Partial(failed_sources) => {
            let time_str = last_updated
                .map(age_str)
                .unwrap_or_else(|| "unknown".to_string());
            let sources = failed_sources.join(", ");
            format!("! {} unreachable (updated {time_str})", sources)
        }
        RefreshState::Failed(err) => format!("refresh failed: {err}"),
        RefreshState::Idle => last_updated
            .map(|t| format!("updated {}", age_str(t)))
            .unwrap_or_default(),
    }
}

fn render_dismiss_modal(frame: &mut ratatui::Frame, input: &tui_input::Input, area: Rect) {
    let modal_width = (area.width * 2 / 3)
        .max(50)
        .min(area.width.saturating_sub(4));
    let modal = popup_area(area, 1, modal_width);

    let block = Block::new()
        .borders(Borders::ALL)
        .title(" Dismiss issue — enter reason (optional) ");
    let input_area = block.inner(modal);

    let scroll = input.visual_scroll(input_area.width.saturating_sub(1) as usize);
    let value = input.value();
    let cursor_pos = input.visual_cursor();

    frame.render_widget(Clear, modal);
    frame.render_widget(block, modal);
    frame.render_widget(
        Paragraph::new(value.chars().skip(scroll).collect::<String>()),
        input_area,
    );
    frame.set_cursor_position((
        input_area.x + (cursor_pos.saturating_sub(scroll)) as u16,
        input_area.y,
    ));
}

pub(crate) fn render(frame: &mut ratatui::Frame, app: &mut App) {
    let [content_area, bar_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    match &mut app.ui.screen {
        Screen::UnifiedList {
            items,
            selected,
            filter,
        } => {
            render_unified(
                frame,
                items,
                *selected,
                filter,
                app.ui.query_input.as_deref(),
                content_area,
            );
        }
        Screen::Detail { parent, view } => {
            detail::render_detail(frame, view, parent, content_area);
        }
        Screen::IssueDetail { issue, scroll, .. } => {
            render_issue_detail(frame, issue, scroll, content_area);
        }
        Screen::PrDetail { pr, scroll, .. } => {
            render_pr_detail(frame, pr, scroll, content_area);
        }
        Screen::DismissingIssue { issue, input, .. } => {
            render_issue_detail(frame, issue, &mut 0, content_area);
            render_dismiss_modal(frame, input, frame.area());
        }
    }

    let right_status =
        right_status_text(&app.data.refresh_state, app.data.last_updated, Utc::now());

    let left = status_bar_left(app);

    let right_width = Span::raw(right_status.as_str()).width() as u16 + 1;
    let [bar_left, bar_right] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(bar_area);
    frame.render_widget(Paragraph::new(format!(" {left}")).style(dim()), bar_left);
    frame.render_widget(
        Paragraph::new(format!("{right_status} ")).style(dim()),
        bar_right,
    );

    if app.ui.show_help {
        let keybinds = match &app.ui.screen {
            Screen::UnifiedList { .. } => KEYBINDS_LIST,
            Screen::Detail { .. } => KEYBINDS_DETAIL,
            Screen::IssueDetail { .. } | Screen::DismissingIssue { .. } => KEYBINDS_ISSUE_READER,
            Screen::PrDetail { .. } => KEYBINDS_PR_READER,
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
    use super::{
        action_hints, position_label, render, right_status_text, status_bar_left, urgency_color,
        urgency_style, wrap_text,
    };
    use crate::display::{Category, DisplayItem, Filter, ListSnapshot};
    use crate::state::{
        App, DataState, DetailView, EnterAction, InvestigateAction, RefreshState, Screen, UiState,
    };
    use chrono::Utc;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::widgets::ListState;
    use ratatui::Terminal;
    use rstest::rstest;
    use workflows::status::StatusItem;

    // ── urgency_color / urgency_style ────────────────────────────────────────

    #[rstest]
    #[case(domain::Urgency::Critical, Color::Red)]
    #[case(domain::Urgency::High, Color::Yellow)]
    #[case(domain::Urgency::Medium, Color::Cyan)]
    #[case(domain::Urgency::Low, Color::Blue)]
    fn urgency_color_maps_each_variant(#[case] urgency: domain::Urgency, #[case] expected: Color) {
        assert_eq!(urgency_color(urgency), expected);
    }

    #[rstest]
    #[case(domain::Urgency::Critical)]
    #[case(domain::Urgency::High)]
    #[case(domain::Urgency::Medium)]
    #[case(domain::Urgency::Low)]
    fn urgency_style_fg_matches_urgency_color(#[case] urgency: domain::Urgency) {
        assert_eq!(urgency_style(urgency).fg, Some(urgency_color(urgency)));
    }

    // ── TestBackend helpers ───────────────────────────────────────────────────

    /// Render `app` into a `width × height` buffer and return it.
    fn draw(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// Render `app1`, then `app2` into the same terminal and return the final buffer.
    /// The second draw sees whatever the first draw left in the buffer — this is
    /// what catches stale-character bugs that only appear after state transitions.
    fn draw_two(
        app1: &mut App,
        app2: &mut App,
        width: u16,
        height: u16,
    ) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, app1)).unwrap();
        terminal.draw(|frame| render(frame, app2)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// Serialise the entire buffer as a multi-line string (one line per row).
    /// Empty cells become spaces so each line is exactly `buf.area.width` chars wide.
    fn screen_text(buf: &ratatui::buffer::Buffer) -> String {
        let w = buf.area.width as usize;
        let h = buf.area.height as usize;
        (0..h)
            .map(|y| {
                buf.content[y * w..(y + 1) * w]
                    .iter()
                    .map(|cell| {
                        if cell.symbol().is_empty() {
                            " "
                        } else {
                            cell.symbol()
                        }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Extract the last (status bar) row of a buffer as a plain string.
    /// Empty cells are rendered as spaces so the snapshot width is always `buf.area.width`.
    fn status_row(buf: &ratatui::buffer::Buffer) -> String {
        let w = buf.area.width as usize;
        let last_y = (buf.area.height - 1) as usize;
        buf.content[last_y * w..(last_y + 1) * w]
            .iter()
            .map(|cell| {
                if cell.symbol().is_empty() {
                    " "
                } else {
                    cell.symbol()
                }
            })
            .collect()
    }

    fn ci_item() -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("ooloth", "hub"),
            workflow_name: "CI".to_string(),
            job_name: Some("check".to_string()),
            step_name: Some("fmt".to_string()),
            error: None,
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/ooloth/hub/actions/runs/25608685802".to_string(),
        })
    }

    fn partial_app() -> App {
        App {
            data: DataState {
                // Use None for last_updated so the timestamp is deterministic ("unknown").
                refresh_state: RefreshState::Partial(vec!["source-a".to_string()]),
                last_updated: None,
                ..DataState::default()
            },
            ..App::default()
        }
    }

    fn idle_app() -> App {
        App::default() // RefreshState::Idle, last_updated: None → right status = ""
    }

    // ── Two-frame status bar snapshot tests ──────────────────────────────────
    //
    // These catch stale-character bugs that only emerge after the right or left
    // status text changes length between frames — the bug pattern in the wild.

    #[test]
    fn status_bar_right_partial_then_idle() {
        // Frame 1: long right text ("! source-a unreachable (updated unknown)")
        // Frame 2: empty right text (Idle + no timestamp → "")
        // Catches stale characters if the longer text isn't fully overwritten.
        let buf = draw_two(&mut partial_app(), &mut idle_app(), 80, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn status_bar_right_idle_then_partial() {
        // Frame 1: empty right text → Frame 2: long right text.
        // Checks that the longer string doesn't overflow its allocated area.
        let buf = draw_two(&mut idle_app(), &mut partial_app(), 80, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn status_bar_left_group_then_single_item() {
        // Frame 1: group selected → "1/2 · Press ↩ to expand 1 items"
        // Frame 2: single CI item selected → longer left text with ↩ URL and i hint.
        // Catches stale characters if the longer left text isn't fully overwritten.
        let group = DisplayItem::Group {
            label: "errors".to_string(),
            items: vec![ci_item()],
        };
        let single = DisplayItem::Single(ci_item());

        let mut app1 = unified_list_app(vec![group]);
        let mut app2 = unified_list_app(vec![single]);
        let buf = draw_two(&mut app1, &mut app2, 120, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn status_bar_left_single_item_then_empty() {
        // Frame 1: CI item selected → long left text with ↩ in it.
        // Frame 2: empty list → left and right are both empty.
        // Isolates whether ↩ causes misalignment that persists into the next frame.
        let mut app1 = unified_list_app(vec![DisplayItem::Single(ci_item())]);
        let mut app2 = unified_list_app(vec![]);
        let buf = draw_two(&mut app1, &mut app2, 120, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    // ── Single-frame baseline snapshots ──────────────────────────────────────
    //
    // Capture what the status bar looks like in key states so regressions are
    // visible as snapshot diffs.

    #[test]
    fn status_bar_single_frame_ci_selected() {
        // CI item selected: left shows position + ↩ URL + i hints; right is empty.
        let mut app = unified_list_app(vec![DisplayItem::Single(ci_item())]);
        let buf = draw(&mut app, 120, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn status_bar_single_frame_partial_state() {
        // Partial refresh: left is empty (no items); right shows the warning.
        let mut app = partial_app();
        let buf = draw(&mut app, 80, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn status_bar_single_frame_in_progress() {
        let mut app = App {
            data: DataState {
                refresh_state: RefreshState::InProgress,
                ..DataState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    fn pr() -> StatusItem {
        StatusItem::Pr(domain::PullRequest {
            number: 1,
            title: "Fix".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/1".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            kind: domain::PrKind::ToReview,
            author: "alice".to_string(),
            review_decision: None,
            review_count: 0,
            head_branch: "feat/fix".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
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
    fn action_hints_open_url() {
        let enter = EnterAction::OpenUrl("https://example.com".to_string());
        let inv = InvestigateAction::None;
        assert_eq!(action_hints(&enter, &inv), " · [↩] open");
    }

    #[test]
    fn action_hints_expand_group() {
        let enter = EnterAction::OpenDetail {
            group_index: 0,
            item_count: 3,
        };
        let inv = InvestigateAction::None;
        assert_eq!(action_hints(&enter, &inv), " · [↩] expand 3 items");
    }

    #[test]
    fn action_hints_investigate_ci() {
        let enter = EnterAction::None;
        let inv = InvestigateAction::LaunchCi {
            repo: "owner/repo".to_string(),
            run_url: "https://example.com".to_string(),
        };
        assert_eq!(action_hints(&enter, &inv), " · [i] investigate");
    }

    #[cfg(feature = "private")]
    #[test]
    fn action_hints_investigate_media() {
        let enter = EnterAction::None;
        let inv = InvestigateAction::LaunchMediaBlocked {
            title: "Show — S01E01".to_string(),
            error: "Invalid video file".to_string(),
        };
        assert_eq!(action_hints(&enter, &inv), " · [i] investigate");
    }

    #[test]
    fn action_hints_combined() {
        let enter = EnterAction::OpenUrl("https://example.com".to_string());
        let inv = InvestigateAction::LaunchCi {
            repo: "owner/repo".to_string(),
            run_url: "https://example.com".to_string(),
        };
        assert_eq!(action_hints(&enter, &inv), " · [↩] open · [i] investigate");
    }

    #[test]
    fn right_status_in_progress() {
        let now = Utc::now();
        assert_eq!(
            right_status_text(&RefreshState::InProgress, None, now),
            "refreshing…"
        );
    }

    #[test]
    fn right_status_failed_shows_error_message() {
        let now = Utc::now();
        assert_eq!(
            right_status_text(
                &RefreshState::Failed("network error".to_string()),
                None,
                now
            ),
            "refresh failed: network error"
        );
    }

    #[test]
    fn right_status_idle_no_timestamp_is_empty() {
        let now = Utc::now();
        assert_eq!(right_status_text(&RefreshState::Idle, None, now), "");
    }

    #[test]
    fn right_status_idle_updated_within_a_minute() {
        let now = Utc::now();
        let last_updated = now - chrono::Duration::seconds(30);
        assert_eq!(
            right_status_text(&RefreshState::Idle, Some(last_updated), now),
            "updated just now"
        );
    }

    #[test]
    fn right_status_idle_updated_minutes_ago() {
        let now = Utc::now();
        let last_updated = now - chrono::Duration::minutes(5);
        assert_eq!(
            right_status_text(&RefreshState::Idle, Some(last_updated), now),
            "updated 5m ago"
        );
    }

    #[test]
    fn right_status_partial_no_timestamp() {
        let now = Utc::now();
        assert_eq!(
            right_status_text(&RefreshState::Partial(vec!["media".to_string()]), None, now),
            "! media unreachable (updated unknown)"
        );
    }

    #[test]
    fn right_status_partial_updated_within_a_minute() {
        let now = Utc::now();
        let last_updated = now - chrono::Duration::seconds(10);
        assert_eq!(
            right_status_text(
                &RefreshState::Partial(vec!["media".to_string()]),
                Some(last_updated),
                now
            ),
            "! media unreachable (updated just now)"
        );
    }

    #[test]
    fn right_status_partial_multiple_sources() {
        let now = Utc::now();
        let last_updated = now - chrono::Duration::minutes(2);
        assert_eq!(
            right_status_text(
                &RefreshState::Partial(vec!["media".to_string(), "linear issues".to_string()]),
                Some(last_updated),
                now
            ),
            "! media, linear issues unreachable (updated 2m ago)"
        );
    }

    // ── Full-screen unified list snapshots ───────────────────────────────────
    //
    // These capture the entire rendered buffer (not just the status bar row) so
    // that regressions in list layout, borders, urgency dividers, and item
    // rendering are visible as snapshot diffs.

    #[test]
    fn full_screen_unified_list_empty() {
        // U1: No items — just the "All" border with an empty body and status bar.
        let mut app = unified_list_app(vec![]);
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_mixed_urgency() {
        // U2: High-urgency CI item + Low-urgency PR → urgency divider between them.
        let mut app = unified_list_app(vec![
            DisplayItem::Single(ci_item()),
            DisplayItem::Single(pr()),
        ]);
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_group_selected() {
        // U3: A group item is selected — shows "↩ to expand" hint in the row.
        let mut app = unified_list_app(vec![DisplayItem::Group {
            label: "hub errors".to_string(),
            items: vec![ci_item()],
        }]);
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_category_filter() {
        // U4: Category filter active — green border + category label in the title.
        let mut app = App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items: vec![DisplayItem::Single(ci_item())],
                    selected: 0,
                    filter: Filter {
                        category: Some(Category::Errors),
                        query: None,
                    },
                },
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_query_input() {
        // U5: Search query being typed — yellow border + query text in the title.
        let mut app = App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items: vec![DisplayItem::Single(ci_item())],
                    selected: 0,
                    filter: Filter::default(),
                },
                query_input: Some("hub".to_string()),
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_narrow_terminal() {
        // U6: 40-column terminal — long item text must wrap onto a second line.
        let mut app = unified_list_app(vec![DisplayItem::Single(ci_item())]);
        let buf = draw(&mut app, 40, 10);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_help_popup() {
        // U7: Help popup overlaid on the list.
        let mut app = App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items: vec![DisplayItem::Single(ci_item())],
                    selected: 0,
                    filter: Filter::default(),
                },
                show_help: true,
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 20);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // ── More full-screen unified list snapshots ───────────────────────────────

    #[test]
    fn full_screen_unified_list_pr_selected() {
        // U8: PR item (Low urgency, no investigate action) selected as item 2/2.
        // Tests that: (a) the inline hint is "↩ to open" only (no "i" hint);
        // (b) the CI item above it shows no hint; (c) position label is "2/2".
        let mut app = App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items: vec![DisplayItem::Single(ci_item()), DisplayItem::Single(pr())],
                    selected: 1,
                    filter: Filter::default(),
                },
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_scrolled() {
        // U9: 15 PR items, last item selected — list must scroll to show it.
        // Tests that the scroll offset logic renders only the visible window
        // and the selected item appears at the bottom of the viewport.
        let items: Vec<DisplayItem> = (0..15).map(|_| DisplayItem::Single(pr())).collect();
        let mut app = App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items,
                    selected: 14,
                    filter: Filter::default(),
                },
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_empty_filter_result() {
        // U10: Category filter active but no items match — green border with
        // filter title, empty body.  Different from U1 (no filter) and U4 (items match).
        let mut app = App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items: vec![],
                    selected: 0,
                    filter: Filter {
                        category: Some(Category::Errors),
                        query: None,
                    },
                },
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_committed_query() {
        // U11: Query committed (filter.query set, query_input None) — green
        // border + query in title. Distinct from U5 (query_input set → yellow border).
        let mut app = App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items: vec![DisplayItem::Single(ci_item())],
                    selected: 0,
                    filter: Filter {
                        category: None,
                        query: Some("hub".to_string()),
                    },
                },
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    fn issue_detail_app(issue: domain::Issue, scroll: u16) -> App {
        App {
            ui: UiState {
                screen: Screen::IssueDetail {
                    parent: ListSnapshot {
                        items: vec![],
                        selected: 0,
                        filter: Filter::default(),
                    },
                    issue,
                    scroll,
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn stub_issue_with_body() -> domain::Issue {
        domain::Issue {
            number: 42,
            title: "Invariant violation in render pipeline".to_string(),
            repo: domain::RepoSlug::new("ooloth", "hub"),
            url: "https://github.com/ooloth/hub/issues/42".to_string(),
            author: "agent".to_string(),
            age: chrono::Duration::days(3),
            urgency: domain::Urgency::Low,
            labels: vec![
                "status:needs-human-review".to_string(),
                "area:render".to_string(),
            ],
            body: Some(
                "## Summary\n\nThe render pipeline does not handle edge cases correctly.\n\n\
                 ## Steps to reproduce\n\n1. Open the TUI\n2. Navigate to the issue list\n\
                 3. Press Enter on an issue\n\n## Expected\n\nThe issue body is displayed.\n\n\
                 ## Actual\n\nThe screen is blank."
                    .to_string(),
            ),
        }
    }

    fn stub_issue_no_body() -> domain::Issue {
        domain::Issue {
            number: 7,
            title: "No description issue".to_string(),
            repo: domain::RepoSlug::new("ooloth", "hub"),
            url: "https://github.com/ooloth/hub/issues/7".to_string(),
            author: "agent".to_string(),
            age: chrono::Duration::days(3),
            urgency: domain::Urgency::Low,
            labels: vec![],
            body: None,
        }
    }

    fn dismissing_app(issue: domain::Issue, draft: &str) -> App {
        let mut input = tui_input::Input::default();
        for c in draft.chars() {
            input.handle(tui_input::InputRequest::InsertChar(c));
        }
        App {
            ui: UiState {
                screen: Screen::DismissingIssue {
                    parent: ListSnapshot {
                        items: vec![],
                        selected: 0,
                        filter: Filter::default(),
                    },
                    issue,
                    input,
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    // ── Full-screen DismissingIssue snapshots ─────────────────────────────────

    #[test]
    fn full_screen_dismissing_issue_empty_prompt() {
        // D1: Dismiss modal open with empty input.
        let mut app = dismissing_app(stub_issue_with_body(), "");
        let buf = draw(&mut app, 120, 30);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_dismissing_issue_with_text() {
        // D2: Dismiss modal open with typed reason.
        let mut app = dismissing_app(stub_issue_with_body(), "Not relevant to this project");
        let buf = draw(&mut app, 120, 30);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // ── Full-screen IssueDetail snapshots ─────────────────────────────────────

    #[test]
    fn full_screen_issue_detail_with_body() {
        // I1: Issue with body and labels at scroll=0.
        let mut app = issue_detail_app(stub_issue_with_body(), 0);
        let buf = draw(&mut app, 80, 20);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_issue_detail_no_body() {
        // I2: Issue with no body and no labels — shows "(no description)" placeholder.
        let mut app = issue_detail_app(stub_issue_no_body(), 0);
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_issue_detail_scrolled() {
        // I3: Same issue as I1 but scroll=3 — body content shifts up.
        let mut app = issue_detail_app(stub_issue_with_body(), 3);
        let buf = draw(&mut app, 80, 20);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn status_bar_in_issue_detail() {
        // I4: Status bar in IssueDetail shows "[a] approve · [o] open · [Esc] back".
        let mut app = issue_detail_app(stub_issue_with_body(), 0);
        let buf = draw(&mut app, 120, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    // ── Full-screen PrDetail snapshots ───────────────────────────────────────

    fn stub_pr_with_body() -> domain::PullRequest {
        domain::PullRequest {
            number: 102,
            title: "Add PrDetail screen".to_string(),
            repo: domain::RepoSlug::new("ooloth", "hub"),
            url: "https://github.com/ooloth/hub/pull/102".to_string(),
            age: chrono::Duration::days(1),
            urgency: domain::Urgency::Medium,
            kind: domain::PrKind::Mine,
            author: "ooloth".to_string(),
            review_decision: None,
            review_count: 2,
            head_branch: "feat/pr-detail".to_string(),
            base_branch: "main".to_string(),
            body: Some(
                "## Summary\n\nAdds a detail screen for PRs.\n\n\
                 ## Why\n\nReadability from terminal without leaving the TUI."
                    .to_string(),
            ),
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
        }
    }

    fn stub_pr_no_body() -> domain::PullRequest {
        domain::PullRequest {
            number: 99,
            title: "Fix typo in README".to_string(),
            repo: domain::RepoSlug::new("ooloth", "hub"),
            url: "https://github.com/ooloth/hub/pull/99".to_string(),
            age: chrono::Duration::hours(5),
            urgency: domain::Urgency::Low,
            kind: domain::PrKind::Mine,
            author: "ooloth".to_string(),
            review_decision: None,
            review_count: 0,
            head_branch: "fix/readme-typo".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
        }
    }

    fn pr_detail_app(pr: domain::PullRequest, scroll: u16) -> App {
        App {
            ui: UiState {
                screen: Screen::PrDetail {
                    parent: ListSnapshot {
                        items: vec![],
                        selected: 0,
                        filter: Filter::default(),
                    },
                    pr,
                    scroll,
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    #[test]
    fn full_screen_pr_detail_with_body() {
        // P1: PR with body and review count at scroll=0.
        let mut app = pr_detail_app(stub_pr_with_body(), 0);
        let buf = draw(&mut app, 80, 20);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_pr_detail_no_body() {
        // P2: PR with no body — shows "(no description)" placeholder.
        let mut app = pr_detail_app(stub_pr_no_body(), 0);
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_pr_detail_scrolled() {
        // P3: PR with body at scroll=2 — content shifts up.
        let mut app = pr_detail_app(stub_pr_with_body(), 2);
        let buf = draw(&mut app, 80, 20);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn status_bar_in_pr_detail() {
        // P4: Status bar shows "[↩] open · [i] investigate · [Esc] back".
        let mut app = pr_detail_app(stub_pr_with_body(), 0);
        let buf = draw(&mut app, 120, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn full_screen_pr_detail_ci_success() {
        // P5: CI success badge + review count in bottom-left.
        let mut pr = stub_pr_with_body();
        pr.ci_status = Some(domain::CiStatus::Success);
        let mut app = pr_detail_app(pr, 0);
        let buf = draw(&mut app, 80, 10);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_pr_detail_ci_failure() {
        // P6: CI failure badge, no reviews.
        let mut pr = stub_pr_no_body();
        pr.ci_status = Some(domain::CiStatus::Failure);
        let mut app = pr_detail_app(pr, 0);
        let buf = draw(&mut app, 80, 10);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_pr_detail_ci_pending() {
        // P7: CI pending badge + 1 review.
        let mut pr = stub_pr_with_body();
        pr.ci_status = Some(domain::CiStatus::Pending);
        pr.review_count = 1;
        let mut app = pr_detail_app(pr, 0);
        let buf = draw(&mut app, 80, 10);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // ── PrDetail diff snapshots ───────────────────────────────────────────────

    fn stub_pr_with_diff() -> domain::PullRequest {
        let mut pr = stub_pr_with_body();
        pr.total_changed_files = 2;
        pr.changed_files = vec![
            domain::ChangedFile {
                path: "src/main.rs".to_string(),
                additions: 3,
                deletions: 1,
                patch: Some(
                    "@@ -10,7 +10,9 @@\n context\n-old line\n+new line\n+another new\n+third new"
                        .to_string(),
                ),
            },
            domain::ChangedFile {
                path: "assets/logo.png".to_string(),
                additions: 0,
                deletions: 0,
                patch: None,
            },
        ];
        pr
    }

    #[test]
    fn full_screen_pr_detail_with_diff() {
        // D1: PR body followed by diff section with one text file and one binary.
        let mut app = pr_detail_app(stub_pr_with_diff(), 0);
        let buf = draw(&mut app, 80, 30);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_pr_detail_diff_truncated() {
        // D2: total_changed_files > changed_files.len() — shows truncation footer.
        let mut pr = stub_pr_with_diff();
        pr.total_changed_files = 150;
        let mut app = pr_detail_app(pr, 0);
        let buf = draw(&mut app, 80, 30);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_pr_detail_diff_scrolled() {
        // D3: scrolled into the diff section.
        let mut app = pr_detail_app(stub_pr_with_diff(), 10);
        let buf = draw(&mut app, 80, 20);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_pr_detail_with_inline_comments() {
        // D4: inline review comment after a changed line; file-level comment at end.
        let mut pr = stub_pr_with_diff();
        pr.review_threads = vec![
            domain::ReviewThread {
                path: "src/main.rs".to_string(),
                line: Some(11), // context line "context" is new-file line 10; "+new line" is 11
                comments: vec![domain::ReviewComment {
                    author: "reviewer".to_string(),
                    age: chrono::Duration::days(2),
                    body: "Why not use a constant here?".to_string(),
                }],
            },
            domain::ReviewThread {
                path: "src/main.rs".to_string(),
                line: None, // file-level
                comments: vec![domain::ReviewComment {
                    author: "reviewer2".to_string(),
                    age: chrono::Duration::hours(3),
                    body: "Overall LGTM.".to_string(),
                }],
            },
        ];
        let mut app = pr_detail_app(pr, 0);
        let buf = draw(&mut app, 80, 40);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // ── Full-screen detail view snapshots ─────────────────────────────────────

    #[test]
    fn full_screen_detail_view_first_selected() {
        // D1: Group expanded, first item selected.
        let items = vec![DisplayItem::Group {
            label: "hub errors".to_string(),
            items: vec![ci_item(), ci_item()],
        }];
        let mut app = detail_app(items, 0, 0);
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_detail_view_last_selected() {
        // D2: Group expanded, last item selected (different scroll position).
        let items = vec![DisplayItem::Group {
            label: "hub errors".to_string(),
            items: vec![ci_item(), ci_item()],
        }];
        let mut app = detail_app(items, 0, 1);
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // ── Status bar gap snapshots ──────────────────────────────────────────────

    #[test]
    fn status_bar_single_frame_failed() {
        // S1: Failed refresh — right side shows "refresh failed: …".
        let mut app = App {
            data: DataState {
                refresh_state: RefreshState::Failed("connection refused".to_string()),
                ..DataState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn status_bar_single_frame_detail_view() {
        // S2: Detail view — position is within the group; enter hint shows the item URL.
        let items = vec![DisplayItem::Group {
            label: "hub errors".to_string(),
            items: vec![ci_item(), ci_item()],
        }];
        let mut app = detail_app(items, 0, 0);
        let buf = draw(&mut app, 120, 5);
        insta::assert_snapshot!(status_row(&buf));
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
