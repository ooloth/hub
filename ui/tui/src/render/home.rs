use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use crate::display::{display_item_line, display_item_urgency, CatData, DisplayItem};
use crate::state::{App, Screen, TILE_COLS};

fn truncate(text: &str, max_width: usize) -> String {
    if text.chars().count() <= max_width {
        text.to_string()
    } else {
        format!(
            "{}…",
            text.chars()
                .take(max_width.saturating_sub(1))
                .collect::<String>()
        )
    }
}

fn render_tile(frame: &mut ratatui::Frame, cat_data: &CatData, focused: bool, area: Rect) {
    let border_style = if focused {
        Style::default().fg(super::FOCUS_COLOR)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };

    let count = cat_data.items.len();
    let title_style = if focused {
        Style::default()
            .fg(super::FOCUS_COLOR)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Reset)
            .remove_modifier(Modifier::DIM)
            .add_modifier(Modifier::BOLD)
    };
    let title = Span::styled(format!(" {} ", cat_data.cat.label()), title_style);
    let block = Block::new()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if count == 0 {
        frame.render_widget(Paragraph::new("(none)").style(super::dim()), inner);
        return;
    }

    let available = inner.height as usize;
    let has_more = count > available;
    let preview_count = if has_more {
        available.saturating_sub(1)
    } else {
        count.min(available)
    };
    let text_width = (inner.width as usize).saturating_sub(2); // "● "

    let mut lines: Vec<Line> = cat_data
        .items
        .iter()
        .take(preview_count)
        .map(|item| {
            let dot_style = super::urgency_style(display_item_urgency(item));
            let (label, count_suffix) = match item {
                DisplayItem::Group { label, items } => {
                    let suffix = format!(" ({})", items.len());
                    let label_width = text_width.saturating_sub(suffix.chars().count());
                    (truncate(label, label_width), Some(suffix))
                }
                DisplayItem::Single(_) => (truncate(&display_item_line(item), text_width), None),
            };
            let mut spans = vec![Span::styled("● ", dot_style), Span::raw(label)];
            if let Some(s) = count_suffix {
                spans.push(Span::styled(s, super::dim()));
            }
            Line::from(spans)
        })
        .collect();

    if has_more {
        let more = count - preview_count;
        lines.push(Line::from(Span::styled(
            format!("  ↓ {more} more"),
            super::dim(),
        )));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

pub(super) fn render_home(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let Screen::Home { focused_tile } = &app.ui.screen else {
        return;
    };
    let focused_tile = *focused_tile;
    let n = app.data.cats.len();
    if n == 0 {
        return;
    }
    let cols = TILE_COLS;
    let rows = n.div_ceil(cols);

    let row_areas = Layout::vertical(
        (0..rows)
            .map(|_| Constraint::Ratio(1, rows as u32))
            .collect::<Vec<_>>(),
    )
    .split(area);

    for (row_idx, &row_area) in row_areas.iter().enumerate() {
        let start = row_idx * cols;
        let end = (start + cols).min(n);
        let count = end - start;
        let col_areas = Layout::horizontal(
            (0..count)
                .map(|_| Constraint::Ratio(1, count as u32))
                .collect::<Vec<_>>(),
        )
        .split(row_area);
        for (col_idx, &col_area) in col_areas.iter().enumerate() {
            let tile_idx = start + col_idx;
            render_tile(
                frame,
                &app.data.cats[tile_idx],
                tile_idx == focused_tile,
                col_area,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{
        backend::TestBackend,
        style::{Color, Modifier},
        Terminal,
    };

    use crate::display::{CatData, Category};
    use crate::state::{App, DataState};

    fn two_cat_app() -> App {
        App {
            data: DataState {
                cats: vec![
                    CatData {
                        cat: Category::Errors,
                        items: vec![],
                    },
                    CatData {
                        cat: Category::Prs,
                        items: vec![],
                    },
                ],
                ..DataState::default()
            },
            ..App::default()
        }
    }

    #[test]
    fn focused_tile_has_green_border() {
        let app = two_cat_app(); // focused_tile: 0 (Errors tile)
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| super::render_home(f, &app, f.area()))
            .unwrap();
        let cell = terminal.backend().buffer().get(0, 0);
        assert_eq!(cell.fg, Color::Green);
    }

    #[test]
    fn unfocused_tile_is_dimmed() {
        let app = two_cat_app(); // focused_tile: 0, so Prs tile (x=40) is unfocused
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| super::render_home(f, &app, f.area()))
            .unwrap();
        let cell = terminal.backend().buffer().get(40, 0);
        assert!(cell.modifier.contains(Modifier::DIM));
    }
}
