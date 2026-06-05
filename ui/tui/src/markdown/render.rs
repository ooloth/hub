// Vendored from tui-markdown 0.3.7 (MIT/Apache-2.0), modified:
//   - SYNTAX_SET uses two-face's extended syntax set (adds TypeScript, TOML, etc.)
//   - Theme uses two-face's built-in CatppuccinMocha instead of base16-ocean.dark
//   - tracing instrumentation removed
//   - pub(crate) visibility throughout

use std::sync::LazyLock;

use ansi_to_tui::IntoText;
use itertools::{Itertools, Position};
use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, CowStr, Event, HeadingLevel, Options as ParseOptions, Parser,
    Tag, TagEnd,
};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span, Text};
use syntect::{
    easy::HighlightLines,
    parsing::SyntaxSet,
    util::{as_24_bit_terminal_escaped, LinesWithEndings},
};
use two_face::theme::EmbeddedThemeName;

use crate::markdown::options::Options;
use crate::markdown::style_sheet::StyleSheet;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

static MOCHA_THEME: LazyLock<syntect::highlighting::Theme> = LazyLock::new(|| {
    two_face::theme::extra()
        .get(EmbeddedThemeName::CatppuccinMocha)
        .clone()
});

pub(crate) fn from_str(input: &str) -> Text<'_> {
    from_str_with_options(input, &Options::default())
}

pub(crate) fn highlight_json(input: &str) -> Vec<Line<'static>> {
    let syntax = SYNTAX_SET
        .find_syntax_by_extension("json")
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    let mut h = HighlightLines::new(syntax, &MOCHA_THEME);
    LinesWithEndings::from(input)
        .filter_map(|line| h.highlight_line(line, &SYNTAX_SET).ok())
        .filter_map(|ranges| as_24_bit_terminal_escaped(&ranges, false).into_text().ok())
        .flat_map(|t| t.lines.into_iter())
        .collect()
}

fn from_str_with_options<'a, S>(input: &'a str, options: &Options<S>) -> Text<'a>
where
    S: StyleSheet,
{
    let mut parse_opts = ParseOptions::empty();
    parse_opts.insert(ParseOptions::ENABLE_STRIKETHROUGH);
    parse_opts.insert(ParseOptions::ENABLE_TASKLISTS);
    parse_opts.insert(ParseOptions::ENABLE_HEADING_ATTRIBUTES);
    parse_opts.insert(ParseOptions::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    let parser = Parser::new_ext(input, parse_opts);

    let mut writer = TextWriter::new(parser, options.styles.clone());
    writer.run();
    writer.text
}

struct HeadingMeta<'a> {
    id: Option<CowStr<'a>>,
    classes: Vec<CowStr<'a>>,
    attrs: Vec<(CowStr<'a>, Option<CowStr<'a>>)>,
}

impl<'a> HeadingMeta<'a> {
    fn into_option(self) -> Option<Self> {
        if self.id.is_some() || !self.classes.is_empty() || !self.attrs.is_empty() {
            Some(self)
        } else {
            None
        }
    }

    fn to_suffix(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(id) = &self.id {
            parts.push(format!("#{id}"));
        }
        for class in &self.classes {
            parts.push(format!(".{class}"));
        }
        for (key, value) in &self.attrs {
            match value {
                Some(v) => parts.push(format!("{key}={v}")),
                None => parts.push(key.to_string()),
            }
        }
        if parts.is_empty() {
            None
        } else {
            Some(format!(" {{{}}}", parts.join(" ")))
        }
    }
}

struct TextWriter<'a, I, S: StyleSheet> {
    iter: I,
    text: Text<'a>,
    inline_styles: Vec<Style>,
    line_prefixes: Vec<Span<'a>>,
    line_styles: Vec<Style>,
    code_highlighter: Option<HighlightLines<'a>>,
    list_indices: Vec<Option<u64>>,
    link: Option<CowStr<'a>>,
    styles: S,
    heading_meta: Option<HeadingMeta<'a>>,
    in_metadata_block: bool,
    needs_newline: bool,
}

impl<'a, I, S> TextWriter<'a, I, S>
where
    I: Iterator<Item = Event<'a>>,
    S: StyleSheet,
{
    fn new(iter: I, styles: S) -> Self {
        Self {
            iter,
            text: Text::default(),
            inline_styles: vec![],
            line_styles: vec![],
            line_prefixes: vec![],
            list_indices: vec![],
            needs_newline: false,
            code_highlighter: None,
            link: None,
            styles,
            heading_meta: None,
            in_metadata_block: false,
        }
    }

    fn run(&mut self) {
        while let Some(event) = self.iter.next() {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: Event<'a>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.text(text),
            Event::Code(code) => self.code(code),
            Event::Html(_) | Event::InlineHtml(_) => {}
            Event::FootnoteReference(_) => {}
            Event::SoftBreak => self.soft_break(),
            Event::HardBreak => self.hard_break(),
            Event::Rule => self.rule(),
            Event::TaskListMarker(checked) => self.task_list_marker(checked),
            Event::InlineMath(_) | Event::DisplayMath(_) => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'a>) {
        match tag {
            Tag::Paragraph => self.start_paragraph(),
            Tag::Heading {
                level,
                id,
                classes,
                attrs,
            } => self.start_heading(level, HeadingMeta { id, classes, attrs }),
            Tag::BlockQuote(kind) => self.start_blockquote(kind),
            Tag::CodeBlock(kind) => self.start_codeblock(kind),
            Tag::List(start_index) => self.start_list(start_index),
            Tag::Item => self.start_item(),
            Tag::Emphasis => self.push_inline_style(Style::new().italic()),
            Tag::Strong => self.push_inline_style(Style::new().bold()),
            Tag::Strikethrough => self.push_inline_style(Style::new().crossed_out()),
            Tag::Subscript | Tag::Superscript => {
                self.push_inline_style(Style::new().dim().italic())
            }
            Tag::Link { dest_url, .. } => self.push_link(dest_url),
            Tag::MetadataBlock(_) => self.start_metadata_block(),
            Tag::Image { .. }
            | Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.end_paragraph(),
            TagEnd::Heading(_) => self.end_heading(),
            TagEnd::BlockQuote(_) => self.end_blockquote(),
            TagEnd::CodeBlock => self.end_codeblock(),
            TagEnd::List(_) => self.end_list(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_inline_style(),
            TagEnd::Subscript | TagEnd::Superscript => self.pop_inline_style(),
            TagEnd::Link => self.pop_link(),
            TagEnd::MetadataBlock(_) => self.end_metadata_block(),
            TagEnd::Item
            | TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::Image
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition => {}
        }
    }

    fn start_paragraph(&mut self) {
        if self.needs_newline {
            self.push_line(Line::default());
        }
        self.push_line(Line::default());
        self.needs_newline = false;
    }

    fn end_paragraph(&mut self) {
        self.needs_newline = true;
    }

    fn start_heading(&mut self, level: HeadingLevel, heading_meta: HeadingMeta<'a>) {
        if self.needs_newline {
            self.push_line(Line::default());
        }
        let heading_level = match level {
            HeadingLevel::H1 => 1,
            HeadingLevel::H2 => 2,
            HeadingLevel::H3 => 3,
            HeadingLevel::H4 => 4,
            HeadingLevel::H5 => 5,
            HeadingLevel::H6 => 6,
        };
        let style = self.styles.heading(heading_level);
        let content = format!("{} ", "#".repeat(heading_level as usize));
        self.push_line(Line::styled(content, style));
        self.heading_meta = heading_meta.into_option();
        self.needs_newline = false;
    }

    fn end_heading(&mut self) {
        if let Some(meta) = self.heading_meta.take() {
            if let Some(suffix) = meta.to_suffix() {
                self.push_span(Span::styled(suffix, self.styles.heading_meta()));
            }
        }
        self.needs_newline = true;
    }

    fn start_blockquote(&mut self, _kind: Option<BlockQuoteKind>) {
        if self.needs_newline {
            self.push_line(Line::default());
            self.needs_newline = false;
        }
        self.line_prefixes.push(Span::from(">"));
        self.line_styles.push(self.styles.blockquote());
    }

    fn end_blockquote(&mut self) {
        self.line_prefixes.pop();
        self.line_styles.pop();
        self.needs_newline = true;
    }

    fn text(&mut self, text: CowStr<'a>) {
        if let Some(highlighter) = &mut self.code_highlighter {
            let highlighted: Text = LinesWithEndings::from(&text)
                .filter_map(|line| highlighter.highlight_line(line, &SYNTAX_SET).ok())
                .filter_map(|part| as_24_bit_terminal_escaped(&part, false).into_text().ok())
                .flatten()
                .collect();

            for line in highlighted.lines {
                self.text.push_line(line);
            }
            self.needs_newline = false;
            return;
        }

        for (position, line) in text.lines().with_position() {
            if self.needs_newline {
                self.push_line(Line::default());
                self.needs_newline = false;
            }
            if matches!(position, Position::Middle | Position::Last) {
                self.push_line(Line::default());
            }
            let style = self.inline_styles.last().copied().unwrap_or_default();
            self.push_span(Span::styled(line.to_owned(), style));
        }
        self.needs_newline = false;
    }

    fn code(&mut self, code: CowStr<'a>) {
        self.push_span(Span::styled(code, self.styles.code()));
    }

    fn hard_break(&mut self) {
        self.push_line(Line::default());
    }

    fn start_metadata_block(&mut self) {
        if self.needs_newline {
            self.push_line(Line::default());
        }
        self.line_styles.push(self.styles.metadata_block());
        self.push_line(Line::from("---"));
        self.push_line(Line::default());
        self.in_metadata_block = true;
    }

    fn end_metadata_block(&mut self) {
        if self.in_metadata_block {
            self.push_line(Line::from("---"));
            self.line_styles.pop();
            self.in_metadata_block = false;
            self.needs_newline = true;
        }
    }

    fn rule(&mut self) {
        if self.needs_newline {
            self.push_line(Line::default());
        }
        self.push_line(Line::from("---"));
        self.needs_newline = true;
    }

    fn start_list(&mut self, index: Option<u64>) {
        if self.list_indices.is_empty() && self.needs_newline {
            self.push_line(Line::default());
        }
        self.list_indices.push(index);
    }

    fn end_list(&mut self) {
        self.list_indices.pop();
        self.needs_newline = true;
    }

    fn start_item(&mut self) {
        self.push_line(Line::default());
        let width = self.list_indices.len() * 4 - 3;
        if let Some(last_index) = self.list_indices.last_mut() {
            let span = match last_index {
                None => Span::from(" ".repeat(width - 1) + "- "),
                Some(index) => {
                    *index += 1;
                    format!("{:width$}. ", *index - 1).light_blue()
                }
            };
            self.push_span(span);
        }
        self.needs_newline = false;
    }

    fn task_list_marker(&mut self, checked: bool) {
        let marker = if checked { 'x' } else { ' ' };
        let marker_span = Span::from(format!("[{marker}] "));
        if let Some(line) = self.text.lines.last_mut() {
            if let Some(first_span) = line.spans.first_mut() {
                let content = first_span.content.to_mut();
                if content.ends_with("- ") {
                    let len = content.len();
                    content.truncate(len - 2);
                    content.push_str("- [");
                    content.push(marker);
                    content.push_str("] ");
                    return;
                }
            }
            line.spans.insert(1, marker_span);
        } else {
            self.push_span(marker_span);
        }
    }

    fn soft_break(&mut self) {
        if self.in_metadata_block {
            self.hard_break();
        } else {
            self.push_span(Span::raw(" "));
        }
    }

    fn start_codeblock(&mut self, kind: CodeBlockKind<'_>) {
        if !self.text.lines.is_empty() {
            self.push_line(Line::default());
        }
        let lang = match kind {
            CodeBlockKind::Fenced(ref lang) => lang.as_ref(),
            CodeBlockKind::Indented => "",
        };
        self.set_code_highlighter(lang);
        let span = Span::from(format!("```{lang}"));
        self.push_line(span.into());
        // Push code style AFTER the opening fence so only content is coloured.
        if self.code_highlighter.is_none() {
            self.line_styles.push(self.styles.code());
        }
        self.needs_newline = true;
    }

    fn end_codeblock(&mut self) {
        // Pop code style BEFORE the closing fence so only content was coloured.
        if self.code_highlighter.is_none() {
            self.line_styles.pop();
        }
        self.push_line(Span::from("```").into());
        self.needs_newline = true;
        self.code_highlighter = None;
    }

    fn set_code_highlighter(&mut self, lang: &str) {
        if !lang.is_empty() {
            if let Some(syntax) = SYNTAX_SET.find_syntax_by_token(lang) {
                self.code_highlighter = Some(HighlightLines::new(syntax, &MOCHA_THEME));
            }
        }
    }

    fn push_inline_style(&mut self, style: Style) {
        let current = self.inline_styles.last().copied().unwrap_or_default();
        self.inline_styles.push(current.patch(style));
    }

    fn pop_inline_style(&mut self) {
        self.inline_styles.pop();
    }

    fn push_line(&mut self, line: Line<'a>) {
        let style = self.line_styles.last().copied().unwrap_or_default();
        let mut line = line.patch_style(style);
        let prefixes: Vec<Span<'a>> = self.line_prefixes.to_vec();
        if !prefixes.is_empty() {
            line.spans.insert(0, " ".into());
        }
        for prefix in prefixes.into_iter().rev() {
            line.spans.insert(0, prefix);
        }
        self.text.lines.push(line);
    }

    fn push_span(&mut self, span: Span<'a>) {
        if let Some(line) = self.text.lines.last_mut() {
            line.push_span(span);
        } else {
            self.push_line(Line::from(vec![span]));
        }
    }

    fn push_link(&mut self, dest_url: CowStr<'a>) {
        self.link = Some(dest_url);
    }

    fn pop_link(&mut self) {
        if let Some(link) = self.link.take() {
            self.push_span(" (".into());
            self.push_span(Span::styled(link, self.styles.link()));
            self.push_span(")".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::style_sheet::CatppuccinStyleSheet;

    fn code_style() -> Style {
        CatppuccinStyleSheet.code()
    }

    fn fence_lines<'a>(text: &'a Text<'a>) -> Vec<&'a ratatui::text::Line<'a>> {
        text.lines
            .iter()
            .filter(|l| l.spans.iter().any(|s| s.content.starts_with("```")))
            .collect()
    }

    fn content_lines<'a>(text: &'a Text<'a>) -> Vec<&'a ratatui::text::Line<'a>> {
        text.lines
            .iter()
            .filter(|l| !l.spans.iter().any(|s| s.content.starts_with("```")))
            .collect()
    }

    #[test]
    fn unlanguaged_fence_content_carries_code_style() {
        let text = from_str("```\nhello\n```\n");
        assert!(
            content_lines(&text).iter().any(|l| l.style == code_style()),
            "expected content lines to carry code() style"
        );
    }

    #[test]
    fn unlanguaged_fence_delimiters_are_unstyled() {
        let text = from_str("```\nhello\n```\n");
        assert!(
            fence_lines(&text)
                .iter()
                .all(|l| l.style == Style::default()),
            "expected fence lines to have no style"
        );
    }

    #[test]
    fn unknown_language_fence_content_carries_code_style() {
        let text = from_str("```xyzzy\nhello\n```\n");
        assert!(
            content_lines(&text).iter().any(|l| l.style == code_style()),
            "expected content lines to carry code() style for unknown language"
        );
    }

    #[test]
    fn known_language_fence_produces_styled_spans() {
        let text = from_str("```rust\nlet x = 1;\n```\n");
        assert!(
            text.lines
                .iter()
                .flat_map(|l| l.spans.iter())
                .any(|s| s.style != Style::default()),
            "expected styled spans for rust fence"
        );
    }
}
