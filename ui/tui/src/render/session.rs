use super::YELLOW;
use crate::display::{fmt_ts, format_age_short};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

pub(crate) fn blocks_to_lines(blocks: &[domain::StreamBlock]) -> Vec<Line<'static>> {
    use domain::StreamBlock;
    const GREEN: Color = Color::Green;
    const RED: Color = Color::Red;
    const CYAN: Color = Color::Cyan;
    let thinking_style = Style::default()
        .add_modifier(Modifier::DIM)
        .add_modifier(Modifier::ITALIC);
    let dim_style = Style::default().add_modifier(Modifier::DIM);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for block in blocks {
        match block {
            StreamBlock::HumanTurn(text) => {
                let mut iter = text.lines();
                if let Some(first) = iter.next() {
                    lines.push(Line::from(vec![
                        Span::styled("> ", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
                        Span::raw(first.to_string()),
                    ]));
                }
                for rest in iter {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::raw(rest.to_string()),
                    ]));
                }
            }
            StreamBlock::AssistantText(text) => {
                for line in text.lines() {
                    lines.push(Line::raw(line.to_string()));
                }
            }
            StreamBlock::AssistantThinking(text) => {
                for line in text.lines() {
                    lines.push(Line::from(Span::styled(line.to_string(), thinking_style)));
                }
            }
            StreamBlock::ToolCall { name, summary } => {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {name}: "), Style::default().fg(GREEN)),
                    Span::styled(summary.clone(), Style::default().fg(GREEN)),
                ]));
            }
            StreamBlock::ToolResult { is_error, content } => {
                let first = content.lines().next().unwrap_or("").to_string();
                let has_more = content.lines().count() > 1;
                let display = if has_more {
                    format!("{first} …")
                } else {
                    first
                };
                if *is_error {
                    lines.push(Line::from(Span::styled(
                        format!("  \u{2717} {display}"),
                        Style::default().fg(RED),
                    )));
                } else {
                    lines.push(Line::from(Span::styled(
                        format!("  \u{2192} {display}"),
                        dim_style,
                    )));
                }
            }
        }
    }
    lines
}

fn render_stream_pane(
    frame: &mut ratatui::Frame,
    blocks: &[domain::StreamBlock],
    scroll: &mut u16,
    area: Rect,
) {
    let lines = blocks_to_lines(blocks);
    let pane_h = area.height as usize;
    let total = lines.len();
    super::shared::clamp_scroll(total, pane_h, scroll);
    let visible: Vec<Line<'static>> = lines
        .into_iter()
        .skip(*scroll as usize)
        .take(pane_h)
        .collect();
    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new("No session data yet.")
                .style(Style::default().add_modifier(Modifier::DIM)),
            area,
        );
    } else {
        frame.render_widget(Paragraph::new(visible), area);
    }
}

fn task_info_lines(task: &domain::Task) -> Vec<Line<'static>> {
    let status_color = match task.status {
        domain::TaskStatus::InReview => YELLOW,
        domain::TaskStatus::Blocked | domain::TaskStatus::Failed => Color::Red,
        domain::TaskStatus::InProgress => Color::Cyan,
        _ => Color::Gray,
    };
    let mut lines: Vec<Line<'static>> = vec![Line::from(Span::styled(
        format!("{} · {}", task.id, task.title),
        Style::default().add_modifier(Modifier::BOLD),
    ))];

    if let Some(desc) = &task.description {
        lines.push(Line::raw(""));
        for line in desc.split('\n') {
            lines.push(Line::raw(line.to_string()));
        }
    }

    lines.extend([
        Line::raw(""),
        Line::from(vec![
            Span::styled("status  ", Style::default().add_modifier(Modifier::DIM)),
            Span::styled(task.status.to_string(), Style::default().fg(status_color)),
        ]),
        Line::from(vec![
            Span::styled("kind    ", Style::default().add_modifier(Modifier::DIM)),
            Span::raw(task.kind.to_string()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("created ", Style::default().add_modifier(Modifier::DIM)),
            Span::raw(fmt_ts(&task.created_at)),
        ]),
        Line::from(vec![
            Span::styled("updated ", Style::default().add_modifier(Modifier::DIM)),
            Span::raw(fmt_ts(&task.updated_at)),
        ]),
        Line::from(vec![
            Span::styled("age     ", Style::default().add_modifier(Modifier::DIM)),
            Span::raw(format_age_short(task.age)),
        ]),
    ]);

    if !task.links.is_empty() {
        lines.push(Line::raw(""));
        for link in &task.links {
            lines.push(Line::raw(link.clone()));
        }
    }

    if !task.comments.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "── comments ──────────────────────",
            Style::default().add_modifier(Modifier::DIM),
        ));
        for comment in &task.comments {
            lines.push(Line::raw(""));
            let ts = fmt_ts(&comment.created_at);
            let author = comment.author.to_string();
            lines.push(Line::styled(
                format!("{ts}  {author}"),
                Style::default().add_modifier(Modifier::DIM),
            ));
            for line in comment.content.split('\n') {
                lines.push(Line::raw(line.to_string()));
            }
        }
    }

    lines
}

pub(crate) fn render_agent_session_detail(
    frame: &mut ratatui::Frame,
    task: &domain::Task,
    blocks: &[domain::StreamBlock],
    scroll: &mut u16,
    area: Rect,
    border_style: Style,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [stream_raw, divider_area, meta_raw] = Layout::horizontal([
        Constraint::Percentage(60),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);

    let stream_area = Rect {
        width: stream_raw.width.saturating_sub(1),
        ..stream_raw
    };
    let meta_area = Rect {
        x: meta_raw.x + 1,
        width: meta_raw.width.saturating_sub(1),
        ..meta_raw
    };

    {
        let buf = frame.buffer_mut();
        for y in inner.y..inner.y + inner.height {
            buf.set_string(divider_area.x, y, "│", border_style);
        }
        buf.set_string(divider_area.x, area.y, "┬", border_style);
        buf.set_string(divider_area.x, area.y + area.height - 1, "┴", border_style);
    }

    render_stream_pane(frame, blocks, scroll, stream_area);
    frame.render_widget(Paragraph::new(task_info_lines(task)), meta_area);
}
