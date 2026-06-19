use super::{LAVENDER, YELLOW};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

pub(crate) fn parse_hunk_new_start(hunk_header: &str) -> Option<u32> {
    hunk_header
        .split_whitespace()
        .find(|t| t.starts_with('+'))
        .and_then(|t| t.trim_start_matches('+').split(',').next())
        .and_then(|n| n.parse().ok())
}

pub(crate) fn comment_lines(comment: &domain::ReviewComment) -> Vec<Line<'static>> {
    let author_style = Style::default().fg(YELLOW).add_modifier(Modifier::ITALIC);
    let body_style = Style::default().fg(YELLOW);
    let age_str = crate::display::format_age_short(comment.age);

    let mut out = vec![Line::styled(
        format!(" @{} · {}", comment.author, age_str),
        author_style,
    )];

    for body_line in comment.body.lines() {
        out.push(Line::styled(format!(" {body_line}"), body_style));
    }

    out.push(Line::from(""));

    out
}

pub(crate) fn render_thread_comments(out: &mut Vec<Line<'static>>, thread: &domain::ReviewThread) {
    for (i, comment) in thread.comments.iter().enumerate() {
        if i == 0 {
            out.push(Line::from(""));
        }
        out.extend(comment_lines(comment));
    }
}

pub(crate) fn pr_diff_lines(pr: &domain::PullRequest, sep_width: usize) -> Vec<Line<'static>> {
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

pub(crate) fn render_pr_detail(
    frame: &mut ratatui::Frame,
    pr: &domain::PullRequest,
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

    let [body_area_raw, divider_area, meta_raw] = Layout::horizontal([
        Constraint::Percentage(60),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);
    let body_area = Rect {
        width: body_area_raw.width.saturating_sub(1),
        ..body_area_raw
    };

    {
        let buf = frame.buffer_mut();
        for y in inner.y..inner.y + inner.height {
            buf.set_string(divider_area.x, y, "│", border_style);
        }
        buf.set_string(divider_area.x, area.y, "┬", border_style);
        buf.set_string(divider_area.x, area.y + area.height - 1, "┴", border_style);
    }

    let meta_area = Rect {
        x: meta_raw.x + 1,
        width: meta_raw.width.saturating_sub(1),
        ..meta_raw
    };

    let body_width = body_area.width as usize;

    let raw_body = pr
        .body
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("(no description)");

    let mut content = crate::markdown::from_str(raw_body);
    content.lines.push(Line::from(""));

    if !pr.pr_comments.is_empty() {
        let label = " top-level comments ";
        let fill_len = body_width.saturating_sub(label.chars().count() + 4);
        let fill = "─".repeat(fill_len);
        let lav = Style::default().fg(LAVENDER);
        content.lines.push(
            Line::from(vec![
                Span::styled(format!("──{label}"), lav),
                Span::styled(fill, lav),
                Span::styled("──".to_string(), lav),
            ])
            .style(Style::default().add_modifier(Modifier::BOLD)),
        );
        content.lines.push(Line::from(""));
        for comment in &pr.pr_comments {
            content.lines.extend(comment_lines(comment));
        }
    }

    let mut diff = pr_diff_lines(pr, body_width);
    content.lines.append(&mut diff);

    super::shared::clamp_scroll(content.lines.len(), body_area.height as usize, scroll);

    frame.render_widget(
        Paragraph::new(content)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .scroll((*scroll, 0)),
        body_area,
    );

    frame.render_widget(
        Paragraph::new(super::pr_detail_columns::pr_right_column_lines(
            pr,
            meta_area.width as usize,
            meta_area.height as usize,
            border_style,
        )),
        meta_area,
    );
}
