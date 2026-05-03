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
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::io;
use tokio::sync::mpsc;
use workflows::status::{StatusItem, StatusReport, SCHEMA_VERSION};

#[cfg(feature = "private")]
mod private;

#[derive(Debug)]
enum DisplayItem {
    Single(StatusItem),
    Group {
        label: String,
        items: Vec<StatusItem>,
    },
}

// Stores an index into App::display_items rather than cloning items.
#[derive(Debug)]
enum View {
    Main,
    Detail {
        group_index: usize,
        list_state: ListState,
    },
}

#[derive(Debug)]
struct App {
    display_items: Vec<DisplayItem>,
    list_state: ListState,
    view: View,
    is_refreshing: bool,
    last_updated: Option<DateTime<Utc>>,
    error: Option<String>,
    show_help: bool,
}

impl App {
    fn active_list_len(&self) -> usize {
        match &self.view {
            View::Main => self.display_items.len(),
            View::Detail { group_index, .. } => match self.display_items.get(*group_index) {
                Some(DisplayItem::Group { items, .. }) => items.len(),
                _ => 0,
            },
        }
    }

    fn move_up(&mut self) {
        if matches!(self.view, View::Main) {
            let sel = self.list_state.selected().unwrap_or(0);
            if sel > 0 {
                self.list_state.select(Some(sel - 1));
            }
        } else if let View::Detail {
            ref mut list_state, ..
        } = self.view
        {
            let sel = list_state.selected().unwrap_or(0);
            if sel > 0 {
                list_state.select(Some(sel - 1));
            }
        }
    }

    fn move_down(&mut self) {
        let len = self.active_list_len();
        if matches!(self.view, View::Main) {
            let sel = self.list_state.selected().unwrap_or(0);
            if len > 0 && sel < len - 1 {
                self.list_state.select(Some(sel + 1));
            }
        } else if let View::Detail {
            ref mut list_state, ..
        } = self.view
        {
            let sel = list_state.selected().unwrap_or(0);
            if len > 0 && sel < len - 1 {
                list_state.select(Some(sel + 1));
            }
        }
    }

    fn selected_url(&self) -> Option<&str> {
        if matches!(self.view, View::Main) {
            let sel = self.list_state.selected().unwrap_or(0);
            match self.display_items.get(sel)? {
                DisplayItem::Single(item) => item_url(item),
                DisplayItem::Group { .. } => None,
            }
        } else if let View::Detail {
            group_index,
            ref list_state,
        } = self.view
        {
            let sel = list_state.selected().unwrap_or(0);
            match self.display_items.get(group_index)? {
                DisplayItem::Group { items, .. } => items.get(sel).and_then(item_url),
                DisplayItem::Single(_) => None,
            }
        } else {
            None
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

fn group_key(_item: &StatusItem) -> Option<String> {
    #[cfg(feature = "private")]
    if let StatusItem::MediaBlocked(b) = _item {
        return Some(b.error.clone());
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

fn urgency_style(u: domain::Urgency) -> Style {
    match u {
        domain::Urgency::Critical => Style::default().fg(Color::Red),
        domain::Urgency::High => Style::default().fg(Color::Yellow),
        domain::Urgency::Medium => Style::default(),
        domain::Urgency::Low => Style::default().add_modifier(Modifier::DIM),
    }
}

const KEYBINDS: &[(&str, &str)] = &[
    ("?", "open help"),
    ("Esc", "close help / detail"),
    ("↑ / k", "up"),
    ("↓ / j", "down"),
    ("Enter", "open URL / expand group"),
    ("q  / Ctrl-C", "quit"),
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
    let width = (content_width + 4).min(area.width); // +2 borders +2 right padding
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

fn render(frame: &mut ratatui::Frame, app: &mut App) {
    let [list_area, bar_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    let text_width = list_area.width.saturating_sub(2) as usize; // 2 for "● "

    if matches!(app.view, View::Main) {
        let selected = app.list_state.selected();
        let items: Vec<ListItem> = app
            .display_items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let dot_style = if selected == Some(i) {
                    Style::default()
                } else {
                    urgency_style(display_item_urgency(item))
                };
                let dot = Span::styled("● ", dot_style);
                build_list_item(dot, wrap_text(&display_item_line(item), text_width))
            })
            .collect();
        let list =
            List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(list, list_area, &mut app.list_state);
    } else if let View::Detail {
        group_index,
        ref mut list_state,
    } = app.view
    {
        let gi = group_index;
        if let Some(DisplayItem::Group { items, .. }) = app.display_items.get(gi) {
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
                    let dot = Span::styled("● ", dot_style);
                    build_list_item(dot, wrap_text(&item_line(item), text_width))
                })
                .collect();
            let list = List::new(list_items)
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            frame.render_stateful_widget(list, list_area, list_state);
        }
    }

    let (position, right_status) = if matches!(app.view, View::Main) {
        let pos = app
            .list_state
            .selected()
            .map(|i| format!("{}/{}", i + 1, app.display_items.len()))
            .unwrap_or_default();
        let status = if app.is_refreshing {
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
        (pos, status)
    } else if let View::Detail {
        group_index,
        ref list_state,
    } = app.view
    {
        let pos = list_state
            .selected()
            .map(|i| format!("{}/{}", i + 1, app.active_list_len()))
            .unwrap_or_default();
        let label = match app.display_items.get(group_index) {
            Some(DisplayItem::Group { label, .. }) => {
                if label.chars().count() > 40 {
                    format!("{}…", label.chars().take(39).collect::<String>())
                } else {
                    label.clone()
                }
            }
            _ => String::new(),
        };
        (pos, label)
    } else {
        (String::new(), String::new())
    };

    let right_width = right_status.chars().count() as u16;
    let [bar_left, bar_right] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(bar_area);
    let dim = Style::default().add_modifier(Modifier::DIM);
    frame.render_widget(Paragraph::new(position).style(dim), bar_left);
    frame.render_widget(Paragraph::new(right_status).style(dim), bar_right);

    if app.show_help {
        let text = format_keybinds(KEYBINDS);
        let lines = KEYBINDS.len() as u16;
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

    // Try to seed from cache on launch.
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

    let display_items = aggregate(initial_items);
    let mut list_state = ListState::default();
    if !display_items.is_empty() {
        list_state.select(Some(0));
    }
    let mut app = App {
        display_items,
        list_state,
        view: View::Main,
        is_refreshing: start_refresh,
        last_updated: initial_updated,
        error: None,
        show_help: false,
    };

    // Channel for background fetch results.
    let (tx, mut rx) = mpsc::channel::<Result<StatusReport>>(1);

    // Spawn an immediate fetch if cache was absent or stale.
    if start_refresh {
        spawn_fetch(&config, tx.clone());
    }

    // Terminal setup.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app, &conn, &config, &tx, &mut rx).await;

    // Always restore terminal, even on error.
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
    // Consume the first tick so it doesn't fire immediately on launch.
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
                        (KeyCode::Esc, _) if matches!(app.view, View::Detail { .. }) => {
                            app.view = View::Main;
                        }
                        _ if app.show_help => {}
                        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => app.move_up(),
                        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => app.move_down(),
                        (KeyCode::Enter, _) => {
                            if matches!(app.view, View::Main) {
                                let sel = app.list_state.selected().unwrap_or(0);
                                match app.display_items.get(sel) {
                                    Some(DisplayItem::Group { items, .. }) => {
                                        let mut detail_state = ListState::default();
                                        if !items.is_empty() {
                                            detail_state.select(Some(0));
                                        }
                                        app.view = View::Detail {
                                            group_index: sel,
                                            list_state: detail_state,
                                        };
                                    }
                                    Some(DisplayItem::Single(_)) => {
                                        if let Some(url) = app.selected_url() {
                                            let _ = open::that_detached(url);
                                        }
                                    }
                                    None => {}
                                }
                            } else if let Some(url) = app.selected_url() {
                                let _ = open::that_detached(url);
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
                        let display_items = aggregate(report.items);
                        let current = app.list_state.selected().unwrap_or(0);
                        let clamped = current.min(display_items.len().saturating_sub(1));
                        app.list_state
                            .select(if display_items.is_empty() { None } else { Some(clamped) });
                        app.display_items = display_items;
                        if matches!(app.view, View::Detail { .. }) {
                            app.view = View::Main;
                        }
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
