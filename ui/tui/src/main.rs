use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use crossterm::{
    event::{Event, EventStream, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::{collections::HashMap, io};
use tokio::sync::mpsc;
use workflows::status::{StatusItem, StatusReport, SCHEMA_VERSION};

#[cfg(feature = "private")]
mod private;

const TILE_COLS: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum Category {
    Prs,
    Issues,
    Errors,
}

impl Category {
    const ALL: [Category; 3] = [Category::Errors, Category::Prs, Category::Issues];

    fn label(self) -> &'static str {
        match self {
            Category::Prs => "PRs",
            Category::Issues => "Issues",
            Category::Errors => "Errors",
        }
    }
}

#[derive(Debug)]
enum DisplayItem {
    Single(StatusItem),
    Group {
        label: String,
        items: Vec<StatusItem>,
    },
}

#[derive(Debug)]
enum View {
    Home,
    Category {
        cat: Category,
        list_state: ListState,
    },
    Detail {
        cat: Category,
        group_index: usize,
        list_state: ListState,
    },
}

#[derive(Debug)]
struct CatData {
    cat: Category,
    items: Vec<DisplayItem>,
}

#[derive(Debug)]
struct App {
    cats: Vec<CatData>,
    focused_tile: usize,
    view: View,
    is_refreshing: bool,
    last_updated: Option<DateTime<Utc>>,
    error: Option<String>,
    show_help: bool,
}

impl App {
    fn active_list_len(&self) -> usize {
        match &self.view {
            View::Home => self.cats.len(),
            View::Category { cat, .. } => self
                .cats
                .iter()
                .find(|c| c.cat == *cat)
                .map(|c| c.items.len())
                .unwrap_or(0),
            View::Detail {
                cat, group_index, ..
            } => self
                .cats
                .iter()
                .find(|c| c.cat == *cat)
                .and_then(|c| c.items.get(*group_index))
                .map(|d| match d {
                    DisplayItem::Group { items, .. } => items.len(),
                    _ => 0,
                })
                .unwrap_or(0),
        }
    }

    fn move_tile_forward(&mut self) {
        let len = self.cats.len();
        if len > 0 {
            self.focused_tile = (self.focused_tile + 1) % len;
        }
    }

    fn move_tile_back(&mut self) {
        let len = self.cats.len();
        if len > 0 {
            self.focused_tile = (self.focused_tile + len - 1) % len;
        }
    }

    fn move_tile_up(&mut self) {
        let len = self.cats.len();
        if len == 0 {
            return;
        }
        let row = self.focused_tile / TILE_COLS;
        let col = self.focused_tile % TILE_COLS;
        if row > 0 {
            let target = (row - 1) * TILE_COLS + col;
            if target < len {
                self.focused_tile = target;
                return;
            }
        }
        self.focused_tile = (self.focused_tile + len - 1) % len;
    }

    fn move_tile_down(&mut self) {
        let len = self.cats.len();
        if len == 0 {
            return;
        }
        let row = self.focused_tile / TILE_COLS;
        let col = self.focused_tile % TILE_COLS;
        let target = (row + 1) * TILE_COLS + col;
        if target < len {
            self.focused_tile = target;
        } else {
            self.focused_tile = (self.focused_tile + 1) % len;
        }
    }

    fn move_tile_left(&mut self) {
        let len = self.cats.len();
        if len == 0 {
            return;
        }
        let col = self.focused_tile % TILE_COLS;
        if col > 0 {
            self.focused_tile -= 1;
        } else {
            self.focused_tile = (self.focused_tile + len - 1) % len;
        }
    }

    fn move_tile_right(&mut self) {
        let len = self.cats.len();
        if len == 0 {
            return;
        }
        let col = self.focused_tile % TILE_COLS;
        if col + 1 < TILE_COLS && self.focused_tile + 1 < len {
            self.focused_tile += 1;
        } else {
            self.focused_tile = (self.focused_tile + 1) % len;
        }
    }

    fn move_up(&mut self) {
        match &mut self.view {
            View::Home => {}
            View::Category { list_state, .. } | View::Detail { list_state, .. } => {
                let sel = list_state.selected().unwrap_or(0);
                if sel > 0 {
                    list_state.select(Some(sel - 1));
                }
            }
        }
    }

    fn move_down(&mut self) {
        let len = self.active_list_len();
        match &mut self.view {
            View::Home => {}
            View::Category { list_state, .. } | View::Detail { list_state, .. } => {
                let sel = list_state.selected().unwrap_or(0);
                if len > 0 && sel < len - 1 {
                    list_state.select(Some(sel + 1));
                }
            }
        }
    }

    fn selected_url(&self) -> Option<&str> {
        match &self.view {
            View::Home => None,
            View::Category { cat, list_state } => {
                let sel = list_state.selected().unwrap_or(0);
                let cd = self.cats.iter().find(|c| c.cat == *cat)?;
                match cd.items.get(sel)? {
                    DisplayItem::Single(item) => item_url(item),
                    DisplayItem::Group { .. } => None,
                }
            }
            View::Detail {
                cat,
                group_index,
                list_state,
            } => {
                let sel = list_state.selected().unwrap_or(0);
                let cd = self.cats.iter().find(|c| c.cat == *cat)?;
                match cd.items.get(*group_index)? {
                    DisplayItem::Group { items, .. } => items.get(sel).and_then(item_url),
                    _ => None,
                }
            }
        }
    }
}

fn item_url(item: &StatusItem) -> Option<&str> {
    match item {
        StatusItem::Pr(pr) => Some(&pr.url),
        StatusItem::Issue(i) => Some(&i.url),
        StatusItem::Ci(c) => Some(&c.url),
        StatusItem::Linear(l) => Some(&l.url),
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(_)
        | StatusItem::MediaMissing(_)
        | StatusItem::MediaHealth(_)
        | StatusItem::MediaBacklog { .. } => None,
    }
}

fn item_line(item: &StatusItem) -> String {
    match item {
        StatusItem::Pr(pr) => format!("{}  {} (#{})", pr.repo, pr.title, pr.number),
        StatusItem::Issue(i) => format!("{}  {} (#{})", i.repo, i.title, i.number),
        StatusItem::Ci(c) => format!("{}  {}  {}", c.repo, c.workflow_name, c.conclusion),
        StatusItem::Linear(l) => format!("{}  {}  [{}]", l.identifier, l.title, l.state),
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => format!("{}  {}", b.title, b.error),
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => format!("{}  aired {}", m.title, m.air_date),
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => h.message.clone(),
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { count } => format!("{count} episodes in backlog"),
    }
}

fn item_urgency(item: &StatusItem) -> domain::Urgency {
    match item {
        StatusItem::Pr(pr) => pr.urgency,
        StatusItem::Issue(i) => i.urgency,
        StatusItem::Ci(c) => c.urgency,
        StatusItem::Linear(l) => l.urgency,
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => b.urgency,
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => m.urgency,
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => h.urgency,
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { .. } => domain::Urgency::Low,
    }
}

fn display_item_line(item: &DisplayItem) -> String {
    match item {
        DisplayItem::Single(s) => item_line(s),
        DisplayItem::Group { label, items } => format!("{}  ({})", label, items.len()),
    }
}

fn display_item_urgency(item: &DisplayItem) -> domain::Urgency {
    match item {
        DisplayItem::Single(s) => item_urgency(s),
        DisplayItem::Group { items, .. } => items
            .first()
            .map(item_urgency)
            .unwrap_or(domain::Urgency::Low),
    }
}

fn item_category(item: &StatusItem) -> Category {
    match item {
        StatusItem::Ci(_) => Category::Errors,
        StatusItem::Pr(_) => Category::Prs,
        StatusItem::Issue(_) | StatusItem::Linear(_) => Category::Issues,
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(_)
        | StatusItem::MediaMissing(_)
        | StatusItem::MediaHealth(_)
        | StatusItem::MediaBacklog { .. } => Category::Errors,
    }
}

fn group_key(_item: &StatusItem) -> Option<String> {
    #[cfg(feature = "private")]
    if let StatusItem::MediaBlocked(b) = _item {
        return Some(format!("Sonarr: {}", b.error));
    }
    None
}

fn aggregate(items: Vec<StatusItem>) -> Vec<DisplayItem> {
    let mut result: Vec<DisplayItem> = vec![];
    for item in items {
        if let Some(key) = group_key(&item) {
            if let Some(DisplayItem::Group {
                items: group_items, ..
            }) = result
                .iter_mut()
                .find(|d| matches!(d, DisplayItem::Group { label, .. } if *label == key))
            {
                group_items.push(item);
                continue;
            }
            result.push(DisplayItem::Group {
                label: key,
                items: vec![item],
            });
        } else {
            result.push(DisplayItem::Single(item));
        }
    }
    result
        .into_iter()
        .map(|d| match d {
            DisplayItem::Group { items, .. } if items.len() == 1 => {
                DisplayItem::Single(items.into_iter().next().unwrap())
            }
            other => other,
        })
        .collect()
}

fn build_cats(items: Vec<StatusItem>) -> Vec<CatData> {
    let mut by_cat: HashMap<Category, Vec<StatusItem>> = HashMap::new();
    for item in items {
        by_cat.entry(item_category(&item)).or_default().push(item);
    }
    Category::ALL
        .iter()
        .map(|&cat| CatData {
            cat,
            items: aggregate(by_cat.remove(&cat).unwrap_or_default()),
        })
        .collect()
}

fn urgency_style(u: domain::Urgency) -> Style {
    match u {
        domain::Urgency::Critical => Style::default().fg(Color::Red),
        domain::Urgency::High => Style::default().fg(Color::Yellow),
        domain::Urgency::Medium => Style::default(),
        domain::Urgency::Low => Style::default().add_modifier(Modifier::DIM),
    }
}

// Computed before mutating app.view, to avoid borrow conflicts.
enum EnterAction {
    None,
    OpenUrl(String),
    OpenCategory {
        cat: Category,
    },
    OpenDetail {
        cat: Category,
        group_index: usize,
        item_count: usize,
    },
}

fn compute_enter_action(app: &App) -> EnterAction {
    match &app.view {
        View::Home => {
            if app.cats.is_empty() {
                return EnterAction::None;
            }
            EnterAction::OpenCategory {
                cat: app.cats[app.focused_tile].cat,
            }
        }
        View::Category { cat, list_state } => {
            let sel = list_state.selected().unwrap_or(0);
            let Some(cd) = app.cats.iter().find(|c| c.cat == *cat) else {
                return EnterAction::None;
            };
            match cd.items.get(sel) {
                Some(DisplayItem::Group { items, .. }) => EnterAction::OpenDetail {
                    cat: *cat,
                    group_index: sel,
                    item_count: items.len(),
                },
                Some(DisplayItem::Single(_)) => app
                    .selected_url()
                    .map(|u| EnterAction::OpenUrl(u.to_string()))
                    .unwrap_or(EnterAction::None),
                None => EnterAction::None,
            }
        }
        View::Detail { .. } => app
            .selected_url()
            .map(|u| EnterAction::OpenUrl(u.to_string()))
            .unwrap_or(EnterAction::None),
    }
}

const KEYBINDS_HOME: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("h", "left (or prev tile)"),
    ("j", "down (or next tile)"),
    ("k", "up (or prev tile)"),
    ("l", "right (or next tile)"),
    ("Tab", "next tile"),
    ("Shift-Tab", "prev tile"),
    ("Enter", "drill into category"),
    ("q / Ctrl-C", "quit"),
];

const KEYBINDS_CATEGORY: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("h", "previous"),
    ("j", "down"),
    ("k", "up"),
    ("l", "next"),
    ("Enter", "open / drill into group"),
    ("Esc", "back to home"),
    ("q / Ctrl-C", "quit"),
];

const KEYBINDS_DETAIL: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("h", "previous"),
    ("j", "down"),
    ("k", "up"),
    ("l", "next"),
    ("Enter", "open URL"),
    ("Esc", "back to category"),
    ("q / Ctrl-C", "quit"),
];

fn format_keybinds(keybinds: &[(&str, &str)]) -> String {
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

fn popup_area(area: Rect, content_lines: u16, content_width: u16) -> Rect {
    let width = (content_width + 4).min(area.width);
    let height = (content_lines + 2).min(area.height);
    Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    )
}

fn build_list_item(dot: Span<'static>, wrapped: Vec<String>) -> ListItem<'static> {
    let mut lines: Vec<Line> = wrapped
        .into_iter()
        .enumerate()
        .map(|(j, chunk)| {
            if j == 0 {
                Line::from(vec![dot.clone(), Span::raw(chunk)])
            } else {
                Line::from(Span::raw(format!("  {chunk}")))
            }
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::from(vec![dot, Span::raw("")]));
    }
    ListItem::new(Text::from(lines))
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    if total <= width {
        return vec![text.to_string()];
    }
    let mut lines = vec![];
    let mut start = 0;
    while start < total {
        let end = (start + width).min(total);
        if end == total {
            lines.push(chars[start..].iter().collect());
            break;
        }
        let split = chars[start..end]
            .iter()
            .rposition(|&c| c == ' ')
            .map(|p| start + p)
            .unwrap_or(end);
        lines.push(chars[start..split].iter().collect());
        start = split + 1;
    }
    lines
}

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
        Style::default().fg(Color::Green)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };

    let count = cat_data.items.len();
    let title_style = if focused {
        Style::default()
            .fg(Color::Green)
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
        frame.render_widget(
            Paragraph::new("(none)").style(Style::default().add_modifier(Modifier::DIM)),
            inner,
        );
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
            let dot_style = urgency_style(display_item_urgency(item));
            Line::from(vec![
                Span::styled("● ", dot_style),
                Span::raw(truncate(&display_item_line(item), text_width)),
            ])
        })
        .collect();

    if has_more {
        let more = count - preview_count;
        lines.push(Line::from(Span::styled(
            format!("  ↓ {more} more"),
            Style::default().add_modifier(Modifier::DIM),
        )));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn render_home(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let n = app.cats.len();
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
                &app.cats[tile_idx],
                tile_idx == app.focused_tile,
                col_area,
            );
        }
    }
}

fn render(frame: &mut ratatui::Frame, app: &mut App) {
    let [content_area, bar_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    if matches!(app.view, View::Home) {
        render_home(frame, app, content_area);
    } else if let View::Category {
        cat,
        ref mut list_state,
    } = app.view
    {
        let cat_items = app
            .cats
            .iter()
            .find(|c| c.cat == cat)
            .map(|c| c.items.as_slice())
            .unwrap_or(&[]);
        let title = Span::styled(
            format!(" {} ", cat.label()),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        );
        let block = Block::new()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Green));
        let inner = block.inner(content_area);
        frame.render_widget(block, content_area);
        let text_width = inner.width.saturating_sub(2) as usize;
        let selected = list_state.selected();
        let list_items: Vec<ListItem> = cat_items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let dot_style = if selected == Some(i) {
                    Style::default()
                } else {
                    urgency_style(display_item_urgency(item))
                };
                build_list_item(
                    Span::styled("● ", dot_style),
                    wrap_text(&display_item_line(item), text_width),
                )
            })
            .collect();
        let list = List::new(list_items)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(list, inner, list_state);
    } else if let View::Detail {
        cat,
        group_index,
        ref mut list_state,
    } = app.view
    {
        let group_data = app
            .cats
            .iter()
            .find(|c| c.cat == cat)
            .and_then(|c| c.items.get(group_index))
            .and_then(|d| {
                if let DisplayItem::Group { label, items } = d {
                    Some((label.as_str(), items.as_slice()))
                } else {
                    None
                }
            });
        if let Some((label, items)) = group_data {
            let title = Span::styled(
                format!(" {} ", label),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            );
            let block = Block::new()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Green));
            let inner = block.inner(content_area);
            frame.render_widget(block, content_area);
            let text_width = inner.width.saturating_sub(2) as usize;
            let selected = list_state.selected();
            let list_items: Vec<ListItem> = items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let dot_style = if selected == Some(i) {
                        Style::default()
                    } else {
                        urgency_style(item_urgency(item))
                    };
                    build_list_item(
                        Span::styled("● ", dot_style),
                        wrap_text(&item_line(item), text_width),
                    )
                })
                .collect();
            let list = List::new(list_items)
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            frame.render_stateful_widget(list, inner, list_state);
        }
    }

    // Status bar
    let right_status = if app.is_refreshing {
        if let Some(err) = &app.error {
            format!("refresh failed: {err}")
        } else {
            "refreshing…".to_string()
        }
    } else if let Some(t) = app.last_updated {
        let mins = (Utc::now() - t).num_minutes();
        if mins == 0 {
            "updated just now".to_string()
        } else {
            format!("updated {mins}m ago")
        }
    } else {
        String::new()
    };

    let left = match &app.view {
        View::Home => {
            let total: usize = app.cats.iter().map(|c| c.items.len()).sum();
            format!("{total} items · home")
        }
        View::Category { cat, list_state } => {
            let n = app
                .cats
                .iter()
                .find(|c| c.cat == *cat)
                .map(|c| c.items.len())
                .unwrap_or(0);
            let pos = list_state
                .selected()
                .map(|i| format!("{}/{n}", i + 1))
                .unwrap_or_default();
            format!("{pos} · {}", cat.label())
        }
        View::Detail {
            cat,
            group_index,
            list_state,
        } => {
            let cd = app.cats.iter().find(|c| c.cat == *cat);
            let (count, label) = match cd.and_then(|c| c.items.get(*group_index)) {
                Some(DisplayItem::Group { items, label }) => (items.len(), label.as_str()),
                _ => (0, ""),
            };
            let pos = list_state
                .selected()
                .map(|i| format!("{}/{count}", i + 1))
                .unwrap_or_default();
            format!("{pos} · {label}")
        }
    };

    let right_width = right_status.chars().count() as u16;
    let [bar_left, bar_right] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(bar_area);
    let dim = Style::default().add_modifier(Modifier::DIM);
    frame.render_widget(Paragraph::new(left).style(dim), bar_left);
    frame.render_widget(Paragraph::new(right_status).style(dim), bar_right);

    if app.show_help {
        let keybinds = match &app.view {
            View::Home => KEYBINDS_HOME,
            View::Category { .. } => KEYBINDS_CATEGORY,
            View::Detail { .. } => KEYBINDS_DETAIL,
        };
        let text = format_keybinds(keybinds);
        let lines = keybinds.len() as u16;
        let width = text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
        let popup = popup_area(frame.area(), lines, width);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(text).block(Block::new().title(" Keybinds ").borders(Borders::ALL)),
            popup,
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::load()?;
    let conn = store::status::connect()?;
    store::status::ensure_table(&conn)?;

    let (initial_items, initial_updated, start_refresh) =
        match store::status::read(&conn).context("failed to read status cache")? {
            Some(cached)
                if cached.schema_version == SCHEMA_VERSION
                    && (Utc::now() - cached.refreshed_at).num_minutes() <= 30 =>
            {
                let report: StatusReport = serde_json::from_str(&cached.payload)
                    .context("failed to deserialize cached status")?;
                (report.items, Some(cached.refreshed_at), false)
            }
            _ => (vec![], None, true),
        };

    let mut app = App {
        cats: build_cats(initial_items),
        focused_tile: 0,
        view: View::Home,
        is_refreshing: start_refresh,
        last_updated: initial_updated,
        error: None,
        show_help: false,
    };

    let (tx, mut rx) = mpsc::channel::<Result<StatusReport>>(1);
    if start_refresh {
        spawn_fetch(&config, tx.clone());
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app, &conn, &config, &tx, &mut rx).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn spawn_fetch(config: &config::Config, tx: mpsc::Sender<Result<StatusReport>>) {
    let github_token = config.github_token.clone();
    let pr_repos = config.github_pr_repos();
    let issue_repos = config.github_open_issue_repos();
    let assigned_repos = config.github_assigned_issue_repos();
    let ci_repos = config.github_ci_repos();
    let linear_token = config.linear_token.clone();
    let private_names = config.private_monitor_workflow_names();

    tokio::spawn(async move {
        let result = workflows::status::run(
            &github_token,
            &pr_repos,
            &issue_repos,
            &assigned_repos,
            &ci_repos,
            linear_token.as_deref(),
            private_names,
        )
        .await;
        let _ = tx.send(result).await;
    });
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    conn: &rusqlite::Connection,
    config: &config::Config,
    tx: &mpsc::Sender<Result<StatusReport>>,
    rx: &mut mpsc::Receiver<Result<StatusReport>>,
) -> Result<()> {
    let mut events = EventStream::new();
    let mut refresh_interval = tokio::time::interval(tokio::time::Duration::from_secs(30 * 60));
    refresh_interval.tick().await;

    loop {
        terminal.draw(|f| render(f, app))?;

        tokio::select! {
            Some(event) = events.next() => {
                if let Event::Key(key) = event.context("terminal event error")? {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('q'), _)
                        | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,

                        (KeyCode::Char('?'), _) => app.show_help = !app.show_help,
                        (KeyCode::Esc, _) if app.show_help => app.show_help = false,

                        // Esc = back one level
                        (KeyCode::Esc, _) if matches!(app.view, View::Detail { .. }) => {
                            let cat = if let View::Detail { cat, .. } = app.view { cat } else { unreachable!() };
                            let len = app.cats.iter().find(|c| c.cat == cat).map(|c| c.items.len()).unwrap_or(0);
                            let mut ls = ListState::default();
                            if len > 0 { ls.select(Some(0)); }
                            app.view = View::Category { cat, list_state: ls };
                        }
                        (KeyCode::Esc, _) if matches!(app.view, View::Category { .. }) => {
                            app.view = View::Home;
                        }

                        _ if app.show_help => {}

                        // Home tile navigation
                        (KeyCode::Tab, _) if matches!(app.view, View::Home) => app.move_tile_forward(),
                        (KeyCode::BackTab, _) if matches!(app.view, View::Home) => app.move_tile_back(),
                        (KeyCode::Right, _) | (KeyCode::Char('l'), _) if matches!(app.view, View::Home) => app.move_tile_right(),
                        (KeyCode::Left, _) | (KeyCode::Char('h'), _) if matches!(app.view, View::Home) => app.move_tile_left(),
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) if matches!(app.view, View::Home) => app.move_tile_down(),
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) if matches!(app.view, View::Home) => app.move_tile_up(),

                        // List navigation
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) | (KeyCode::Char('h'), _) => app.move_up(),
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) | (KeyCode::Char('l'), _) => app.move_down(),

                        // Enter = drill in or open URL
                        (KeyCode::Enter, _) => {
                            let action = compute_enter_action(app);
                            match action {
                                EnterAction::None => {}
                                EnterAction::OpenUrl(url) => {
                                    let _ = open::that_detached(url);
                                }
                                EnterAction::OpenCategory { cat } => {
                                    let len = app.cats.iter().find(|c| c.cat == cat).map(|c| c.items.len()).unwrap_or(0);
                                    let mut ls = ListState::default();
                                    if len > 0 { ls.select(Some(0)); }
                                    app.view = View::Category { cat, list_state: ls };
                                }
                                EnterAction::OpenDetail { cat, group_index, item_count } => {
                                    let mut ds = ListState::default();
                                    if item_count > 0 { ds.select(Some(0)); }
                                    app.view = View::Detail { cat, group_index, list_state: ds };
                                }
                            }
                        }

                        _ => {}
                    }
                }
            }
            _ = refresh_interval.tick() => {
                if !app.is_refreshing {
                    app.is_refreshing = true;
                    app.error = None;
                    spawn_fetch(config, tx.clone());
                }
            }
            Some(result) = rx.recv() => {
                match result {
                    Ok(report) => {
                        let json = serde_json::to_string(&report)
                            .context("failed to serialize status report")?;
                        store::status::upsert(conn, &json, SCHEMA_VERSION)
                            .context("failed to upsert status cache")?;
                        let cats = build_cats(report.items);
                        app.focused_tile = app.focused_tile.min(cats.len().saturating_sub(1));
                        app.cats = cats;
                        app.view = View::Home;
                        app.last_updated = Some(Utc::now());
                        app.is_refreshing = false;
                        app.error = None;
                    }
                    Err(e) => {
                        app.is_refreshing = false;
                        app.error = Some(e.to_string());
                    }
                }
            }
        }
    }

    Ok(())
}
