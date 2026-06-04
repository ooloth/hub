use anyhow::{Context, Result};
use chrono::Utc;
use crossterm::{
    event::{Event, EventStream},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use secrecy::ExposeSecret;
use std::{io, time::Duration};
use tokio::sync::mpsc;
use workflows::status::{StatusReport, SCHEMA_VERSION};

use crate::display::{build_unified, flatten, Filter};
use crate::input::key_to_action;
use crate::render::render;
use crate::state::{
    handle_msg, App, DataState, DetailMode, Effect, Msg, PrOwnership, RefreshState, ReviewSkill,
    Screen, UiState,
};
use std::collections::HashSet;

mod display;
mod input;
mod investigations;
mod markdown;
mod render;
mod state;

#[cfg(feature = "private")]
mod private;

const REFRESH_INTERVAL_SECS: u64 = 30 * 60;

fn refresh_interval_chrono() -> chrono::Duration {
    chrono::Duration::seconds(REFRESH_INTERVAL_SECS as i64)
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
    let config = config::Config::load().await?;
    let conn = store::status::connect()?;
    store::status::ensure_table(&conn)?;

    let (initial_items, initial_updated, start_refresh) =
        match store::status::read(&conn).context("failed to read status cache")? {
            Some(cached)
                if cached.schema_version == SCHEMA_VERSION
                    && (Utc::now() - cached.refreshed_at) < refresh_interval_chrono() =>
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
            stream_blocks: Vec::new(),
            stream_session_id: None,
        },
        ui: UiState {
            screen: Screen::UnifiedList {
                flat_rows: flatten(&initial_display, &HashSet::new()),
                items: initial_display,
                selected: 0,
                filter: initial_filter,
                expanded_groups: HashSet::new(),
                detail_mode: DetailMode::Hidden,
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
        issue_repos: config.github_issue_repos(),
        ci_repos: config.github_ci_repos(),
        linear_token: config.linear_token.clone(),
        private_workflow_names: config.private_monitor_workflow_names(),
        loki_envs: config.loki_envs(),
        gcp_envs: config.gcp_envs(),
        extra_credentials: config.extra_credentials.clone(),
    };

    tokio::spawn(async move {
        let _ = tx.send(workflows::status::run(params).await).await;
    });
}

fn spawn_git_fetch(config: &config::Config) {
    let projects: Vec<(String, String)> = config
        .projects
        .iter()
        .map(|p| (p.name.clone(), p.repo.clone()))
        .collect();
    tokio::spawn(async move {
        if let Err(e) = workflows::fetch::run(&projects).await {
            eprintln!("hub fetch: {e}");
        }
    });
}

fn request_refresh(
    app: &mut App,
    config: &config::Config,
    tx: &mpsc::Sender<Result<StatusReport>>,
    include_git_fetch: bool,
    delay: Option<Duration>,
) {
    if let Some(d) = delay {
        let params = workflows::status::StatusParams {
            github_token: config.github_token.clone(),
            github_username: config.github_username.clone(),
            pr_repos: config.github_pr_repos(),
            issue_repos: config.github_issue_repos(),
            ci_repos: config.github_ci_repos(),
            linear_token: config.linear_token.clone(),
            private_workflow_names: config.private_monitor_workflow_names(),
            loki_envs: config.loki_envs(),
            gcp_envs: config.gcp_envs(),
            extra_credentials: config.extra_credentials.clone(),
        };
        let git_params = include_git_fetch.then(|| {
            config
                .projects
                .iter()
                .map(|p| (p.name.clone(), p.repo.clone()))
                .collect::<Vec<(String, String)>>()
        });
        let tx = tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(d).await;
            let _ = tx.send(workflows::status::run(params).await).await;
            if let Some(projects) = git_params {
                if let Err(e) = workflows::fetch::run(&projects).await {
                    eprintln!("hub fetch: {e}");
                }
            }
        });
    } else {
        if matches!(app.data.refresh_state, RefreshState::InProgress) {
            return;
        }
        app.data.refresh_state = RefreshState::InProgress;
        spawn_fetch(config, tx.clone());
        if include_git_fetch {
            spawn_git_fetch(config);
        }
    }
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
    let mut refresh_interval =
        tokio::time::interval(tokio::time::Duration::from_secs(REFRESH_INTERVAL_SECS));
    refresh_interval.tick().await;
    // Wakes the loop every minute so the "updated Xm ago" timestamp
    // advances without requiring a keypress.
    let mut display_interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
    display_interval.tick().await;
    // Live-polls the JSONL stream for the selected AgentSession while detail is open.
    let mut stream_interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
    stream_interval.tick().await;
    let (stream_tx, mut stream_rx) = mpsc::channel::<Vec<domain::StreamBlock>>(1);

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
            _ = refresh_interval.tick() => {
                let cached = store::status::read_if_fresh(conn, refresh_interval_chrono())
                    .context("failed to read status cache on tick")?;
                match cached {
                    Some(cached) if cached.schema_version == SCHEMA_VERSION => {
                        let report: StatusReport = serde_json::from_str(&cached.payload)
                            .context("failed to deserialize cached status on tick")?;
                        handle_msg(app, Msg::AppliedFromCache { report, refreshed_at: cached.refreshed_at })?
                    }
                    _ => handle_msg(app, Msg::Tick)?,
                }
            }
            _ = display_interval.tick() => vec![], // redraw only; no state change
            Some(result) = rx.recv() => handle_msg(app, Msg::FetchResult(result))?,
            _ = stream_interval.tick() => {
                if let Screen::UnifiedList {
                    flat_rows,
                    selected,
                    detail_mode: DetailMode::Visible { .. },
                    ..
                } = &app.ui.screen
                {
                    if let Some(crate::display::FlatRow::Single(
                        workflows::status::StatusItem::AgentSession(task),
                    )) = flat_rows.get(*selected)
                    {
                        if let Some(session_id) = &task.session_id {
                            if app.data.stream_session_id.as_deref() != Some(session_id.as_str()) {
                                app.data.stream_blocks.clear();
                                app.data.stream_session_id = Some(session_id.clone());
                            }
                            let cwd = std::env::current_dir()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let sid = session_id.clone();
                            let stx = stream_tx.clone();
                            tokio::spawn(async move {
                                if let Ok(blocks) =
                                    workflows::tasks::read_session_stream(&cwd, &sid).await
                                {
                                    let _ = stx.send(blocks).await;
                                }
                            });
                        }
                    }
                }
                vec![]
            }
            Some(blocks) = stream_rx.recv() => handle_msg(app, Msg::StreamUpdate(blocks))?,
        };

        for effect in effects {
            match effect {
                Effect::Quit => break 'run,
                Effect::OpenUrl(url) => {
                    let _ = open::that_detached(url);
                }
                Effect::OpenPrDiffInDelta { repo, number } => {
                    let window_name = format!("{repo}#{number}-diff");
                    let cmd = format!("gh pr diff {number} -R {repo} | delta; read",);
                    let _ = std::process::Command::new("tmux")
                        .args(["new-window", "-n", &window_name, &cmd])
                        .spawn();
                }
                Effect::LaunchCi { repo, run_url } => {
                    if let Err(err) = investigations::launch(
                        investigations::ci::config(&repo, &run_url),
                        investigations::WorktreeSpec::EphemeralFresh { repo },
                        config,
                    )
                    .await
                    {
                        app.ui.flash = Some(err.to_string());
                    }
                }
                Effect::LaunchIssue { repo, number } => {
                    if let Err(err) = investigations::launch(
                        investigations::issue::config(&repo, number),
                        investigations::WorktreeSpec::EphemeralFresh { repo },
                        config,
                    )
                    .await
                    {
                        app.ui.flash = Some(err.to_string());
                    }
                }
                Effect::LaunchPr {
                    repo,
                    number,
                    kind,
                    review_decision,
                    head_branch,
                    ..
                } => {
                    let ownership = PrOwnership::from_kind(kind);
                    let skill = if review_decision == Some(domain::ReviewDecision::ChangesRequested)
                    {
                        ReviewSkill::PrCommentsConverge
                    } else {
                        ReviewSkill::Converge
                    };
                    if let Err(err) = investigations::launch(
                        investigations::pr::review_config(number, &repo, ownership, skill),
                        investigations::WorktreeSpec::PullRequest {
                            repo,
                            number,
                            head_branch,
                        },
                        config,
                    )
                    .await
                    {
                        app.ui.flash = Some(err.to_string());
                    }
                }
                Effect::ReviewPr {
                    repo,
                    number,
                    ownership,
                    skill,
                    head_branch,
                    ..
                } => {
                    if let Err(err) = investigations::launch(
                        investigations::pr::review_config(number, &repo, ownership, skill),
                        investigations::WorktreeSpec::PullRequest {
                            repo,
                            number,
                            head_branch,
                        },
                        config,
                    )
                    .await
                    {
                        app.ui.flash = Some(err.to_string());
                    }
                }
                Effect::OpenInOcto {
                    repo,
                    number,
                    head_branch,
                } => {
                    if let Err(err) =
                        investigations::open_in_octo(&repo, number, &head_branch, config).await
                    {
                        app.ui.flash = Some(err.to_string());
                    }
                }
                Effect::OpenInLazygit {
                    repo,
                    number,
                    head_branch,
                } => {
                    if let Err(err) =
                        investigations::open_in_lazygit(&repo, number, &head_branch, config).await
                    {
                        app.ui.flash = Some(err.to_string());
                    }
                }
                Effect::LaunchGcp {
                    project,
                    env,
                    title,
                    message,
                    line,
                    url,
                    lookback,
                    gcp_project,
                } => {
                    if let Err(err) = investigations::launch(
                        investigations::gcp::config(
                            &project,
                            &env,
                            &title,
                            &message,
                            &line,
                            &url,
                            &lookback,
                            &gcp_project,
                        ),
                        investigations::WorktreeSpec::Ephemeral { project },
                        config,
                    )
                    .await
                    {
                        app.ui.flash = Some(err.to_string());
                    }
                }
                Effect::LaunchLoki {
                    project,
                    env,
                    title,
                    message,
                    line,
                    url,
                    lookback,
                } => {
                    if let Err(err) = investigations::launch(
                        investigations::loki::config(
                            &project, &env, &title, &message, &line, &url, &lookback,
                        ),
                        investigations::WorktreeSpec::Ephemeral { project },
                        config,
                    )
                    .await
                    {
                        app.ui.flash = Some(err.to_string());
                    }
                }
                #[cfg(feature = "private")]
                Effect::LaunchMediaBlocked { title, error } => {
                    let result = match investigations::media::config(
                        &title,
                        &error,
                        &config.extra_credentials,
                    ) {
                        Ok(cfg) => {
                            investigations::launch(
                                cfg,
                                investigations::WorktreeSpec::CurrentDir,
                                config,
                            )
                            .await
                        }
                        Err(e) => Err(e),
                    };
                    if let Err(err) = result {
                        app.ui.flash = Some(err.to_string());
                    }
                }
                Effect::SetIssueLabels {
                    repo,
                    number,
                    labels,
                } => {
                    match clients::github::set_issue_labels(
                        config.github_token.expose_secret(),
                        &repo,
                        number,
                        &labels,
                    )
                    .await
                    {
                        Ok(()) => {
                            app.ui.flash = Some(format!("Marked #{number} ready for agent"));
                            request_refresh(app, config, tx, false, None);
                        }
                        Err(e) => {
                            app.ui.flash =
                                Some(format!("Could not mark #{number} ready for agent: {e}"));
                        }
                    }
                }
                Effect::MergePullRequest { repo, number } => {
                    match clients::github::merge_pull_request(
                        config.github_token.expose_secret(),
                        &repo,
                        number,
                    )
                    .await
                    {
                        Ok(()) => {
                            app.ui.flash = Some(format!("Merged #{number}"));
                            request_refresh(app, config, tx, true, Some(Duration::from_secs(5)));
                        }
                        Err(e) => {
                            app.ui.flash = Some(format!("Could not merge #{number}: {e}"));
                        }
                    }
                }
                Effect::DismissIssue {
                    repo,
                    number,
                    reason,
                    labels,
                } => {
                    match clients::github::dismiss_issue(
                        config.github_token.expose_secret(),
                        &repo,
                        number,
                        &reason,
                        &labels,
                    )
                    .await
                    {
                        Ok(()) => {
                            app.ui.flash = Some(format!("Dismissed #{number}"));
                            request_refresh(app, config, tx, false, None);
                        }
                        Err(e) => {
                            app.ui.flash = Some(format!("Could not dismiss #{number}: {e}"));
                        }
                    }
                }
                Effect::UpdateTaskStatus { id, status } => {
                    match workflows::tasks::update_status(&id, status) {
                        Ok(()) => {
                            app.ui.flash = Some(format!("{id} → {status}"));
                            request_refresh(app, config, tx, false, None);
                        }
                        Err(e) => {
                            app.ui.flash = Some(format!("Could not update {id}: {e}"));
                        }
                    }
                }
                Effect::StartRefresh => {
                    request_refresh(app, config, tx, true, None);
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
