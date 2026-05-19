use ratatui::style::{Color, Style};

pub(crate) trait StyleSheet: Clone + Send + Sync + 'static {
    fn heading(&self, level: u8) -> Style;
    fn code(&self) -> Style;
    fn link(&self) -> Style;
    fn blockquote(&self) -> Style;
    fn heading_meta(&self) -> Style;
    fn metadata_block(&self) -> Style;
}

// Catppuccin Mocha mauve — matches FOCUS_COLOR in render/mod.rs
const MAUVE: Color = Color::Rgb(203, 166, 247);

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CatppuccinStyleSheet;

impl StyleSheet for CatppuccinStyleSheet {
    fn heading(&self, level: u8) -> Style {
        match level {
            1 => Style::new().fg(MAUVE).bold().underlined(),
            2 => Style::new().fg(MAUVE).bold(),
            3 => Style::new().fg(MAUVE).bold().italic(),
            _ => Style::new().fg(MAUVE).italic(),
        }
    }

    fn code(&self) -> Style {
        Style::new().fg(MAUVE)
    }

    fn link(&self) -> Style {
        Style::new().blue().underlined()
    }

    fn blockquote(&self) -> Style {
        Style::new().dim()
    }

    fn heading_meta(&self) -> Style {
        Style::new().dim()
    }

    fn metadata_block(&self) -> Style {
        Style::new().dim()
    }
}
