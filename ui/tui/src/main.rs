use anyhow::{bail, Context, Result};
use chrono::Utc;
use crossterm::{
    event::{Event, EventStream, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, widgets::ListState, Terminal};
use std::{io, path::Path};
use tokio::sync::mpsc;
use workflows::status::{StatusReport, SCHEMA_VERSION};

use crate::display::build_cats;
use crate::render::render;
use crate::state::{
    compute_enter_action, compute_investigate_action, App, EnterAction, InvestigateAction, View,
};

mod display;
mod render;
mod state;

#[cfg(feature = "private")]
mod private;

fn ci_investigation_command(repo: &str, run_url: &str) -> String {
    format!("claude '/github-ci-investigate {repo} {run_url}'")
}

fn launch_ci_investigation(repo: &str, run_url: &str, cwd: &Path) -> Result<()> {
    if std::env::var("TMUX").is_err() {
        bail!("not in tmux; investigation requires a tmux session");
    }

    let command = ci_investigation_command(repo, run_url);
    let status = std::process::Command::new("tmux")
        .args(["split-window", "-h", "-c"])
        .arg(cwd)
        .arg(command)
        .status()
        .context("failed to start tmux split-window")?;

    if !status.success() {
        bail!("tmux split-window failed with {status}");
    }

    Ok(())
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalSession {
    fn start() -> Result<Self> {
        enable_raw_mode()?;

        let mut stdout = io::stdout();
        if let Err(err) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(err.into());
        }

        let backend = CrosstermBackend::new(stdout);
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(err) => {
                let mut stdout = io::stdout();
                let _ = execute!(stdout, LeaveAlternateScreen);
                let _ = disable_raw_mode();
                return Err(err.into());
            }
        };

        Ok(Self { terminal })
    }

    fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<io::Stdout>> {
        &mut self.terminal
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
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
        flash: None,
    };

    let (tx, mut rx) = mpsc::channel::<Result<StatusReport>>(1);
    if start_refresh {
        spawn_fetch(&config, tx.clone());
    }
    spawn_git_fetch(&config);

    let mut terminal = TerminalSession::start()?;

    run_loop(
        terminal.terminal_mut(),
        &mut app,
        &conn,
        &config,
        &tx,
        &mut rx,
    )
    .await
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

fn spawn_git_fetch(config: &config::Config) {
    let github_token = config.github_token.clone();
    let projects: Vec<(String, String)> = config
        .projects
        .iter()
        .map(|p| (p.name.clone(), p.repo.clone()))
        .collect();
    tokio::spawn(async move {
        if let Err(e) = workflows::fetch::run(&projects, &github_token).await {
            eprintln!("hub fetch: {e}");
        }
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
                    app.flash = None;
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

                        // i = launch investigation skill for selected item
                        (KeyCode::Char('i'), _) => {
                            match compute_investigate_action(app) {
                                InvestigateAction::LaunchCi { repo, run_url } => {
                                    let cwd = std::env::current_dir()
                                        .context("failed to resolve current directory")?;
                                    if let Err(err) = launch_ci_investigation(&repo, &run_url, &cwd)
                                    {
                                        app.flash = Some(err.to_string());
                                    }
                                }
                                InvestigateAction::None => {
                                    app.flash = Some("No investigation mapped".to_string());
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
                    spawn_git_fetch(config);
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

#[cfg(test)]
mod tests {
    use super::ci_investigation_command;

    #[test]
    fn ci_investigation_command_passes_skill_and_context_as_one_prompt() {
        assert_eq!(
            ci_investigation_command(
                "ooloth/hub",
                "https://github.com/ooloth/hub/actions/runs/123"
            ),
            "claude '/github-ci-investigate ooloth/hub https://github.com/ooloth/hub/actions/runs/123'"
        );
    }
}
