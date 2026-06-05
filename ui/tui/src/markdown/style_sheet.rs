use ratatui::style::{Color, Style, Stylize};

pub(crate) trait StyleSheet: Clone + Send + Sync + 'static {
    fn heading(&self, level: u8) -> Style;
    fn code(&self) -> Style;
    fn link(&self) -> Style;
    fn blockquote(&self) -> Style;
    fn heading_meta(&self) -> Style;
    fn metadata_block(&self) -> Style;
}

const TEAL: Color = Color::Rgb(148, 226, 213);
const LAVENDER: Color = Color::Rgb(180, 190, 254);

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CatppuccinStyleSheet;

impl StyleSheet for CatppuccinStyleSheet {
    fn heading(&self, level: u8) -> Style {
        match level {
            1 => Style::new().fg(TEAL).bold().underlined(),
            2 => Style::new().fg(TEAL).bold(),
            3 => Style::new().fg(TEAL).bold().italic(),
            _ => Style::new().fg(TEAL).italic(),
        }
    }

    fn code(&self) -> Style {
        Style::new().fg(LAVENDER)
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
