use super::FOCUS_COLOR;
use crate::display::format_age_short;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph},
};

pub(crate) fn render_issue_detail(
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
    let total_lines =
        super::status_bar::issue_body_line_count(issue.body.as_deref(), inner_width) + 2;
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
