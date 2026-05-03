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
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Terminal,
};
use std::io;
use tokio::sync::mpsc;
use workflows::status::{StatusItem, StatusReport, SCHEMA_VERSION};

#[cfg(feature = "private")]
mod private;

#[derive(Debug)]
struct App {
    items: Vec<StatusItem>,
    selected: usize,
    is_refreshing: bool,
    last_updated: Option<DateTime<Utc>>,
    error: Option<String>,
    show_help: bool,
}

impl App {
    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn move_down(&mut self) {
        if !self.items.is_empty() && self.selected < self.items.len() - 1 {
            self.selected += 1;
        }
    }

    fn selected_url(&self) -> Option<&str> {
        self.items.get(self.selected).and_then(item_url)
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
        StatusItem::Pr(pr) => format!(
            "[{}]  {}  {} (#{})",
            urgency_label(pr.urgency),
            pr.repo,
            pr.title,
            pr.number
        ),
        StatusItem::Issue(i) => format!(
            "[{}]  {}  {} (#{})",
            urgency_label(i.urgency),
            i.repo,
            i.title,
            i.number
        ),
        StatusItem::Ci(c) => format!(
            "[{}]  {}  {}  {}",
            urgency_label(c.urgency),
            c.repo,
            c.workflow_name,
            c.conclusion
        ),
        StatusItem::Linear(l) => format!(
            "[{}]  {}  {}  [{}]",
            urgency_label(l.urgency),
            l.identifier,
            l.title,
            l.state
        ),
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => {
            format!("[{}]  {}  {}", urgency_label(b.urgency), b.title, b.error)
        }
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => format!(
            "[{}]  {}  aired {}",
            urgency_label(m.urgency),
            m.title,
            m.air_date
        ),
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => {
            format!("[{}]  {}", urgency_label(h.urgency), h.message)
        }
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { count } => format!(
            "[{}]  {count} episodes in backlog",
            urgency_label(domain::Urgency::Low)
        ),
    }
}

fn urgency_label(u: domain::Urgency) -> &'static str {
    match u {
        domain::Urgency::Critical => "crit",
        domain::Urgency::High => "high",
        domain::Urgency::Medium => "med ",
        domain::Urgency::Low => "low ",
    }
}

fn popup_area(area: Rect) -> Rect {
    let width = 30u16.min(area.width);
    let height = 6u16.min(area.height);
    Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    )
}

fn render(frame: &mut ratatui::Frame, app: &App) {
    let [list_area, bar_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    let items: Vec<ListItem> = app
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let line = item_line(item);
            if i == app.selected {
                ListItem::new(line).style(Style::default().add_modifier(Modifier::REVERSED))
            } else {
                ListItem::new(line)
            }
        })
        .collect();

    frame.render_widget(List::new(items), list_area);

    let status = if app.is_refreshing {
        if let Some(err) = &app.error {
            format!("refresh failed: {err}")
        } else {
            "refreshing…".to_string()
        }
    } else if let Some(t) = app.last_updated {
        let mins = (Utc::now() - t).num_minutes();
        if mins == 0 {
            "last updated just now".to_string()
        } else {
            format!(
                "last updated {mins}m ago  ·  {n} items",
                n = app.items.len()
            )
        }
    } else {
        String::new()
    };

    frame.render_widget(
        Paragraph::new(status).style(Style::default().add_modifier(Modifier::DIM)),
        bar_area,
    );

    if app.show_help {
        let popup = popup_area(frame.area());
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(
                "  ?  / Esc    close\n  ↑  / ↓      navigate\n  Enter        open URL\n  q  / Ctrl-C  quit",
            )
            .block(Block::new().title(" Keybinds ").borders(Borders::ALL)),
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

    let mut app = App {
        items: initial_items,
        selected: 0,
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
                        _ if app.show_help => {}
                        (KeyCode::Up, _) => app.move_up(),
                        (KeyCode::Down, _) => app.move_down(),
                        (KeyCode::Enter, _) => {
                            if let Some(url) = app.selected_url() {
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
                        app.selected = app.selected.min(report.items.len().saturating_sub(1));
                        app.items = report.items;
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
