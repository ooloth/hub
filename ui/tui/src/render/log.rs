use super::{dim, FOCUS_COLOR};
use crate::display::{LogDetailView, LogLine};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Text},
    widgets::{Block, BorderType, Borders, Paragraph},
};

pub(crate) fn render_log_detail(
    frame: &mut ratatui::Frame,
    view: &LogDetailView,
    scroll: &mut u16,
    area: Rect,
) {
    let (title, source, project, env, message, lines) = match view {
        LogDetailView::Gcp {
            title,
            project,
            env,
            message,
            lines,
            ..
        } => (title, "GCP", project, env, message, lines),
        LogDetailView::Loki {
            title,
            project,
            env,
            message,
            lines,
            ..
        } => (title, "Loki", project, env, message, lines),
    };
    let title = format!(" {} ", title);
    let subtitle = format!(" {} · {}:{} · {} ", source, project, env, message);

    let bold = Style::default().add_modifier(Modifier::BOLD);
    let inner_width = area.width.saturating_sub(2) as usize;

    let mut all_lines: Vec<Line<'static>> = Vec::new();
    for (i, log_line) in lines.iter().enumerate() {
        if i > 0 {
            all_lines.push(Line::from(""));
        }
        match log_line {
            LogLine::Json(v) => {
                let pretty = serde_json::to_string_pretty(v).unwrap_or_else(|_| format!("{v}"));
                all_lines.extend(crate::markdown::highlight_json(&pretty));
            }
            LogLine::Raw(s) => {
                all_lines.push(Line::styled("(not valid JSON — showing raw text)", dim()));
                all_lines.extend(
                    super::unified::wrap_text(s, inner_width.max(1))
                        .into_iter()
                        .map(Line::from),
                );
            }
        }
    }

    super::shared::clamp_scroll(
        all_lines.len(),
        area.height.saturating_sub(2) as usize,
        scroll,
    );

    let block = Block::default()
        .title(Line::from(title).style(bold))
        .title_bottom(Line::from(subtitle).style(dim()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(FOCUS_COLOR));

    let body: Vec<Line<'static>> = all_lines.into_iter().skip(*scroll as usize).collect();

    frame.render_widget(Paragraph::new(Text::from(body)).block(block), area);
}
