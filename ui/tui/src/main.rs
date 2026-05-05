use anyhow::{bail, Context, Result};
use chrono::Utc;
use crossterm::{
    event::{Event, EventStream},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, path::Path};
use tokio::sync::mpsc;
use workflows::status::{StatusReport, SCHEMA_VERSION};

use crate::display::build_cats;
use crate::input::{handle_key, Effect};
use crate::render::render;
use crate::state::{App, View};

mod display;
mod input;
mod render;
mod state;

#[cfg(feature = "private")]
mod private;

fn ci_investigation_command(repo: &str, run_url: &str) -> String {
    format!("claude  --dangerously-skip-permissions '/github-ci-investigate {repo} {run_url}'")
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
        views: vec![View::Home],
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
                    match handle_key(&mut *app, key) {
                        Effect::Quit => break,
                        Effect::OpenUrl(url) => { let _ = open::that_detached(url); }
                        Effect::LaunchCi { repo, run_url } => {
                            let cwd = std::env::current_dir()
                                .context("failed to resolve current directory")?;
                            if let Err(err) = launch_ci_investigation(&repo, &run_url, &cwd) {
                                app.flash = Some(err.to_string());
                            }
                        }
                        Effect::None => {}
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
                        app.views.truncate(1);
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
            "claude  --dangerously-skip-permissions '/github-ci-investigate ooloth/hub https://github.com/ooloth/hub/actions/runs/123'"
        );
    }
}
