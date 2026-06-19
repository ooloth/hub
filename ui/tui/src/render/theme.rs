use ratatui::style::{Color, Modifier, Style};

pub(crate) const FOCUS_COLOR: Color = Color::Rgb(203, 166, 247); // Catppuccin Mocha Mauve
pub(crate) const LAVENDER: Color = Color::Rgb(180, 190, 254); // Catppuccin Mocha Lavender
pub(crate) const YELLOW: Color = Color::Rgb(249, 226, 175); // Catppuccin Mocha Yellow
pub(super) const SELECTION_BG: Color = Color::Rgb(41, 45, 62);

pub(crate) fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub(crate) fn list_highlight() -> Style {
    Style::default()
        .bg(SELECTION_BG)
        .add_modifier(Modifier::BOLD)
}
