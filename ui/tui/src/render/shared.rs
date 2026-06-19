use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Span,
};

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

pub(crate) fn bullet_span(color: Color) -> Span<'static> {
    Span::styled(" · ", Style::default().fg(color))
}

pub(crate) fn segment_chars(segments: &[String]) -> usize {
    segments.iter().map(|s| s.chars().count()).sum::<usize>() + segments.len().saturating_sub(1) * 3
}

pub(crate) fn push_segments(
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

pub(crate) const KEYBINDS_LIST: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("k / j", "up / down"),
    ("gg / G", "go to top / bottom"),
    ("Ctrl-u / Ctrl-d", "page up / down"),
    ("h / l", "collapse / expand group"),
    ("Enter", "open"),
    ("i", "investigate"),
    ("n", "new task (seeded from selected row)"),
    ("N", "new task (blank)"),
    ("p / e / o", "filter PRs / Errors / Issues"),
    ("/", "search"),
    ("a / Esc", "clear filter"),
    ("r", "refresh"),
    ("q / Ctrl-C", "quit"),
];

pub(crate) const KEYBINDS_PR_READER: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("k / j", "scroll up / down"),
    ("gg / G", "go to top / bottom"),
    ("Ctrl-u / Ctrl-d", "page up / down"),
    ("Enter", "open in browser"),
    ("i", "investigate PR"),
    ("o", "open in octo"),
    ("l", "open in lazygit"),
    ("v", "review"),
    ("m", "squash and merge"),
    ("r", "refresh"),
    ("Esc", "back to list"),
    ("q / Ctrl-C", "quit"),
];

pub(crate) fn format_keybinds(keybinds: &[(&str, &str)]) -> String {
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

pub(crate) fn clamp_scroll(total: usize, viewport_height: usize, scroll: &mut u16) {
    let max_scroll = u16::try_from(
        total
            .saturating_sub(viewport_height)
            .min(usize::from(u16::MAX)),
    )
    .unwrap_or(u16::MAX);
    *scroll = (*scroll).min(max_scroll);
}

pub(crate) fn popup_area(area: Rect, content_lines: u16, content_width: u16) -> Rect {
    let width = (content_width + 4).min(area.width);
    let height = (content_lines + 2).min(area.height);
    Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    )
}
