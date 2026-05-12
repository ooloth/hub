use anyhow::{Context, Result};
use chrono::Utc;
use crossterm::{
    event::{Event, EventStream},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, path::PathBuf};
use tokio::sync::mpsc;
use workflows::status::{StatusReport, SCHEMA_VERSION};

use crate::display::{build_unified, Filter};
use crate::input::key_to_action;
use crate::render::render;
use crate::state::{handle_msg, App, DataState, Effect, Msg, RefreshState, Screen, UiState};

mod display;
mod input;
mod investigations;
mod render;
mod state;

#[cfg(feature = "private")]
mod private;

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

    let initial_filter = Filter::default();
    let initial_display = build_unified(initial_items.clone(), &initial_filter);

    let mut app = App {
        data: DataState {
            raw_items: initial_items,
            refresh_state: if start_refresh {
                RefreshState::InProgress
            } else {
                RefreshState::Idle
            },
            last_updated: initial_updated,
        },
        ui: UiState {
            screen: Screen::UnifiedList {
                items: initial_display,
                selected: 0,
                filter: initial_filter,
            },
            ..UiState::default()
        },
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
    let params = workflows::status::StatusParams {
        github_token: config.github_token.clone(),
        github_username: config.github_username.clone(),
        pr_repos: config.github_pr_repos(),
        issue_repos: config.github_open_issue_repos(),
        assigned_issue_repos: config.github_assigned_issue_repos(),
        ci_repos: config.github_ci_repos(),
        linear_token: config.linear_token.clone(),
        private_workflow_names: config.private_monitor_workflow_names(),
        loki_envs: config.loki_envs(),
    };

    tokio::spawn(async move {
        let _ = tx.send(workflows::status::run(params).await).await;
    });
}

async fn resolve_investigation_cwd(config: &config::Config, repo: &str) -> Result<PathBuf, String> {
    let name = config
        .projects
        .iter()
        .find(|p| p.repo == repo)
        .map(|p| p.name.as_str())
        .ok_or_else(|| format!("No project found for {repo}"))?;

    let repos = workflows::fetch::repos_dir();
    let bare = repos.join(name);

    if !bare.exists() {
        return Err("Not fetched yet; run hub fetch".to_string());
    }

    if let Some(wt) = workflows::fetch::default_branch_worktree(&repos, name) {
        return Ok(wt);
    }

    workflows::fetch::ensure_default_branch_worktree(&bare)
        .await
        .map_err(|e| format!("Failed to create worktree: {e}"))?;

    workflows::fetch::default_branch_worktree(&repos, name)
        .ok_or_else(|| "Worktree creation succeeded but path not found".to_string())
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
    // Wakes the loop every minute so the "updated Xm ago" timestamp
    // advances without requiring a keypress.
    let mut display_interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
    display_interval.tick().await;

    'run: loop {
        terminal.draw(|f| render(f, app))?;

        let effects: Vec<Effect> = tokio::select! {
            Some(event) = events.next() => {
                if let Event::Key(key) = event.context("terminal event error")? {
                    if let Some(action) = key_to_action(app, key) {
                        handle_msg(app, Msg::Action(action))?
                    } else {
                        app.ui.pending_g = false;
                        vec![]
                    }
                } else {
                    vec![]
                }
            }
            _ = refresh_interval.tick() => handle_msg(app, Msg::Tick)?,
            _ = display_interval.tick() => vec![], // redraw only; no state change
            Some(result) = rx.recv() => handle_msg(app, Msg::FetchResult(result))?,
        };

        for effect in effects {
            match effect {
                Effect::Quit => break 'run,
                Effect::OpenUrl(url) => {
                    let _ = open::that_detached(url);
                }
                Effect::LaunchCi { repo, run_url } => {
                    match resolve_investigation_cwd(config, &repo).await {
                        Ok(cwd) => {
                            if let Err(err) = investigations::launch(
                                investigations::ci::config(&repo, &run_url),
                                &cwd,
                            ) {
                                app.ui.flash = Some(err.to_string());
                            }
                        }
                        Err(msg) => app.ui.flash = Some(msg),
                    }
                }
                Effect::LaunchIssue { repo, number } => {
                    match resolve_investigation_cwd(config, &repo).await {
                        Ok(cwd) => {
                            if let Err(err) = investigations::launch(
                                investigations::issue::config(&repo, number),
                                &cwd,
                            ) {
                                app.ui.flash = Some(err.to_string());
                            }
                        }
                        Err(msg) => app.ui.flash = Some(msg),
                    }
                }
                Effect::LaunchLoki {
                    project,
                    env,
                    title,
                    message,
                    line,
                } => match std::env::current_dir() {
                    Ok(cwd) => {
                        if let Err(err) = investigations::launch(
                            investigations::loki::config(&project, &env, &title, &message, &line),
                            &cwd,
                        ) {
                            app.ui.flash = Some(err.to_string());
                        }
                    }
                    Err(e) => {
                        app.ui.flash = Some(format!("Cannot determine working directory: {e}"));
                    }
                },
                #[cfg(feature = "private")]
                Effect::LaunchMediaBlocked { title, error } => match std::env::current_dir() {
                    Ok(cwd) => {
                        if let Err(err) = investigations::launch(
                            investigations::media::config(&title, &error),
                            &cwd,
                        ) {
                            app.ui.flash = Some(err.to_string());
                        }
                    }
                    Err(e) => {
                        app.ui.flash = Some(format!("Cannot determine working directory: {e}"));
                    }
                },
                Effect::StartRefresh => {
                    spawn_fetch(config, tx.clone());
                    spawn_git_fetch(config);
                }
                Effect::WriteCache(json) => {
                    store::status::upsert(conn, &json, SCHEMA_VERSION)
                        .context("failed to upsert status cache")?;
                }
            }
        }
    }

    Ok(())
}
