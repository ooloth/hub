//! Hub TUI — the human-facing terminal dashboard for signal triage and task management.
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
    chrono::Duration::seconds(i64::try_from(REFRESH_INTERVAL_SECS).unwrap_or(i64::MAX))
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

    const fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<io::Stdout>> {
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
    let conn = store::status_cache::connect()?;
    store::status_cache::ensure_table(&conn)?;

    let (initial_items, initial_updated, start_refresh) =
        match store::status_cache::read(&conn).context("failed to read status cache")? {
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
            available_repos: config
                .projects
                .iter()
                .filter_map(|p| p.repo.parse().ok())
                .collect(),
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

    drop(tokio::spawn(async move {
        let _ = tx.send(workflows::status::run(params).await).await;
    }));
}

/// Spawns a background task that drives automatic task-state transitions
/// (completion, crash, stall, self-heal) and reaps idle tmux windows.
/// Results arrive via `tasks_tx`.
fn spawn_session_state_poll(tasks_tx: mpsc::Sender<Vec<domain::Task>>) {
    let now = chrono::Utc::now();
    drop(tokio::spawn(async move {
        if let Ok(tasks) = workflows::agent_session::poll_sessions(now) {
            if let Err(e) = workflows::dispatch::reap_idle_windows(&tasks, now).await {
                eprintln!("reap error: {e}");
            }
            let _ = tasks_tx.send(tasks).await;
        }
    }));
}

/// Spawns a background task to read the selected agent session's JSONL stream,
/// if the detail pane is open on an `AgentSession` row. Updates stream state on
/// session change and sends new blocks via `stream_tx`.
fn try_stream_selected_session(app: &mut App, stream_tx: &mpsc::Sender<Vec<domain::StreamBlock>>) {
    let (cwd, session_id) = {
        let Screen::UnifiedList {
            flat_rows,
            selected,
            detail_mode: DetailMode::Visible { .. },
            ..
        } = &app.ui.screen
        else {
            return;
        };
        let Some(crate::display::FlatRow::Single(workflows::status::StatusItem::AgentSession(
            task,
        ))) = flat_rows.get(*selected)
        else {
            return;
        };
        let Some(session_id) = &task.session_id else {
            return;
        };
        // The session JSONL lives under the task's worktree, not the TUI's cwd.
        let cwd = workflows::dispatch::task_workspace_path(task)
            .to_string_lossy()
            .to_string();
        (cwd, session_id.clone())
    };
    if app.data.stream_session_id.as_deref() != Some(session_id.as_str()) {
        app.data.stream_blocks.clear();
        app.data.stream_session_id = Some(session_id.clone());
    }
    let stx = stream_tx.clone();
    drop(tokio::spawn(async move {
        if let Ok(blocks) = workflows::tasks::read_session_stream(&cwd, &session_id).await {
            let _ = stx.send(blocks).await;
        }
    }));
}

fn spawn_git_fetch(config: &config::Config) {
    let projects: Vec<(String, String)> = config
        .projects
        .iter()
        .map(|p| (p.name.clone(), p.repo.clone()))
        .collect();
    drop(tokio::spawn(async move {
        if let Err(e) = workflows::fetch::run(&projects).await {
            eprintln!("hub fetch: {e}");
        }
    }));
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
        drop(tokio::spawn(async move {
            tokio::time::sleep(d).await;
            let _ = tx.send(workflows::status::run(params).await).await;
            if let Some(projects) = git_params {
                if let Err(e) = workflows::fetch::run(&projects).await {
                    eprintln!("hub fetch: {e}");
                }
            }
        }));
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

#[allow(clippy::future_not_send)] // conn holds rusqlite internals that are not Sync
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
    // Consume the first tick so the interval fires after a full period.
    let _ = refresh_interval.tick().await;
    // Wakes the loop every minute so the "updated Xm ago" timestamp
    // advances without requiring a keypress.
    let mut display_interval = tokio::time::interval(tokio::time::Duration::from_mins(1));
    let _ = display_interval.tick().await;
    // Live-polls the JSONL stream for the selected AgentSession while detail is open.
    let mut stream_interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
    let _ = stream_interval.tick().await;
    // Claims the oldest ready task and spawns its agent session.
    let mut dispatch_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
    let _ = dispatch_interval.tick().await;
    let (stream_tx, mut stream_rx) = mpsc::channel::<Vec<domain::StreamBlock>>(1);
    let (tasks_tx, mut tasks_rx) = mpsc::channel::<Vec<domain::Task>>(1);

    'run: loop {
        let _ = terminal.draw(|f| render(f, app))?;

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
            _ = refresh_interval.tick() => on_refresh_tick(app, conn)?,
            _ = display_interval.tick() => vec![], // redraw only; no state change
            Some(result) = rx.recv() => handle_msg(app, Msg::FetchResult(result))?,
            _ = stream_interval.tick() => {
                spawn_session_state_poll(tasks_tx.clone());
                try_stream_selected_session(app, &stream_tx);
                vec![]
            }
            Some(blocks) = stream_rx.recv() => handle_msg(app, Msg::StreamUpdate(blocks))?,
            Some(tasks) = tasks_rx.recv() => handle_msg(app, Msg::TasksPatched(tasks))?,
            _ = dispatch_interval.tick() => {
                if let Err(e) = workflows::dispatch::dispatch().await {
                    eprintln!("dispatch error: {e}");
                }
                vec![]
            }
        };

        for effect in effects {
            if handle_effect(effect, app, conn, config, tx).await? {
                break 'run;
            }
        }
    }

    Ok(())
}

fn on_refresh_tick(app: &mut App, conn: &rusqlite::Connection) -> Result<Vec<Effect>> {
    let cached = store::status_cache::read_if_fresh(conn, refresh_interval_chrono())
        .context("failed to read status cache on tick")?;
    match cached {
        Some(cached) if cached.schema_version == SCHEMA_VERSION => {
            let report: StatusReport = serde_json::from_str(&cached.payload)
                .context("failed to deserialize cached status on tick")?;
            handle_msg(
                app,
                Msg::AppliedFromCache {
                    report,
                    refreshed_at: cached.refreshed_at,
                },
            )
        }
        _ => handle_msg(app, Msg::Tick),
    }
}

/// Processes a single effect. Returns `true` if the loop should quit.
#[allow(clippy::future_not_send)] // conn holds rusqlite internals that are not Sync
async fn handle_effect(
    effect: Effect,
    app: &mut App,
    conn: &rusqlite::Connection,
    config: &config::Config,
    tx: &mpsc::Sender<Result<StatusReport>>,
) -> Result<bool> {
    match effect {
        Effect::Quit => return Ok(true),
        Effect::OpenUrl(url) => {
            let _ = open::that_detached(url);
        }
        Effect::OpenPrDiffInDelta { repo, number } => open_pr_diff(&repo, number),
        Effect::SetIssueLabels {
            repo,
            number,
            labels,
        } => {
            handle_set_issue_labels(app, config, tx, repo, number, labels).await;
        }
        Effect::MergePullRequest { repo, number } => {
            handle_merge_pull_request(app, config, tx, repo, number).await;
        }
        Effect::UpdateTaskStatus { id, status } => {
            handle_update_task_status(app, config, tx, &id, status);
        }
        Effect::CreateTask {
            title,
            description,
            kind,
            links,
            repo,
            origin,
        } => {
            handle_create_task(
                app,
                conn,
                config,
                tx,
                &title,
                description.as_deref(),
                kind,
                &links,
                repo.as_ref(),
                &origin,
            );
        }
        Effect::StartRefresh => request_refresh(app, config, tx, true, None),
        Effect::WriteCache(json) => {
            store::status_cache::upsert(conn, &json, SCHEMA_VERSION)
                .context("failed to upsert status cache")?;
        }
        other => handle_investigation_effect(other, app, config).await,
    }
    Ok(false)
}

/// Dispatches investigation/launch effects that only need `app` and `config`.
async fn handle_investigation_effect(effect: Effect, app: &mut App, config: &config::Config) {
    match effect {
        Effect::LaunchCi { repo, run_url } => handle_launch_ci(app, config, repo, run_url).await,
        Effect::LaunchIssue { repo, number } => {
            handle_launch_issue(app, config, repo, number).await;
        }
        Effect::LaunchPr {
            repo,
            number,
            kind,
            review_decision,
            head_branch,
            ..
        } => {
            handle_launch_pr(
                app,
                config,
                repo,
                number,
                kind,
                review_decision,
                head_branch,
            )
            .await;
        }
        Effect::ReviewPr {
            repo,
            number,
            ownership,
            skill,
            head_branch,
            ..
        } => {
            handle_review_pr(app, config, repo, number, ownership, skill, head_branch).await;
        }
        Effect::OpenInOcto {
            repo,
            number,
            head_branch,
        } => {
            handle_open_in_octo(app, config, repo, number, head_branch).await;
        }
        Effect::OpenInLazygit {
            repo,
            number,
            head_branch,
        } => {
            handle_open_in_lazygit(app, config, repo, number, head_branch).await;
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
            handle_launch_gcp(
                app,
                config,
                project,
                env,
                title,
                message,
                line,
                url,
                lookback,
                gcp_project,
            )
            .await;
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
            handle_launch_loki(
                app, config, project, env, title, message, line, url, lookback,
            )
            .await;
        }
        #[cfg(feature = "private")]
        Effect::LaunchMediaBlocked { title, error } => {
            handle_launch_media(app, config, title, error).await;
        }
        _ => {}
    }
}

fn open_pr_diff(repo: &str, number: u64) {
    let window_name = format!("{repo}#{number}-diff");
    let cmd = format!("gh pr diff {number} -R {repo} | delta; read");
    let _ = std::process::Command::new("tmux")
        .args(["new-window", "-n", &window_name, &cmd])
        .spawn();
}

fn handle_update_task_status(
    app: &mut App,
    config: &config::Config,
    tx: &mpsc::Sender<Result<StatusReport>>,
    id: &domain::TaskId,
    status: domain::TaskStatus,
) {
    match workflows::tasks::update_status(id, status) {
        Ok(()) => {
            app.ui.flash = Some(format!("{id} → {status}"));
            request_refresh(app, config, tx, false, None);
        }
        Err(e) => {
            app.ui.flash = Some(format!("Could not update {id}: {e}"));
        }
    }
}

async fn handle_launch_ci(app: &mut App, config: &config::Config, repo: String, run_url: String) {
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

async fn handle_launch_issue(app: &mut App, config: &config::Config, repo: String, number: u64) {
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

#[allow(clippy::too_many_arguments)]
async fn handle_launch_pr(
    app: &mut App,
    config: &config::Config,
    repo: String,
    number: u64,
    kind: domain::PrKind,
    review_decision: Option<domain::ReviewDecision>,
    head_branch: String,
) {
    let ownership = PrOwnership::from_kind(kind);
    let skill = if review_decision == Some(domain::ReviewDecision::ChangesRequested) {
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

#[allow(clippy::too_many_arguments)]
async fn handle_review_pr(
    app: &mut App,
    config: &config::Config,
    repo: String,
    number: u64,
    ownership: PrOwnership,
    skill: ReviewSkill,
    head_branch: String,
) {
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

#[allow(clippy::too_many_arguments)]
async fn handle_launch_gcp(
    app: &mut App,
    config: &config::Config,
    project: String,
    env: String,
    title: String,
    message: String,
    line: String,
    url: String,
    lookback: String,
    gcp_project: String,
) {
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

#[allow(clippy::too_many_arguments)]
async fn handle_launch_loki(
    app: &mut App,
    config: &config::Config,
    project: String,
    env: String,
    title: String,
    message: String,
    line: String,
    url: String,
    lookback: String,
) {
    if let Err(err) = investigations::launch(
        investigations::loki::config(&project, &env, &title, &message, &line, &url, &lookback),
        investigations::WorktreeSpec::Ephemeral { project },
        config,
    )
    .await
    {
        app.ui.flash = Some(err.to_string());
    }
}

async fn handle_open_in_octo(
    app: &mut App,
    config: &config::Config,
    repo: String,
    number: u64,
    head_branch: String,
) {
    if let Err(err) = investigations::open_in_octo(&repo, number, &head_branch, config).await {
        app.ui.flash = Some(err.to_string());
    }
}

async fn handle_open_in_lazygit(
    app: &mut App,
    config: &config::Config,
    repo: String,
    number: u64,
    head_branch: String,
) {
    if let Err(err) = investigations::open_in_lazygit(&repo, number, &head_branch, config).await {
        app.ui.flash = Some(err.to_string());
    }
}

#[cfg(feature = "private")]
async fn handle_launch_media(app: &mut App, config: &config::Config, title: String, error: String) {
    let result = match investigations::media::config(&title, &error, &config.extra_credentials) {
        Ok(cfg) => {
            investigations::launch(cfg, investigations::WorktreeSpec::CurrentDir, config).await
        }
        Err(e) => Err(e),
    };
    if let Err(err) = result {
        app.ui.flash = Some(err.to_string());
    }
}

async fn handle_set_issue_labels(
    app: &mut App,
    config: &config::Config,
    tx: &mpsc::Sender<Result<StatusReport>>,
    repo: String,
    number: u64,
    labels: Vec<String>,
) {
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
            app.ui.flash = Some(format!("Could not mark #{number} ready for agent: {e}"));
        }
    }
}

async fn handle_merge_pull_request(
    app: &mut App,
    config: &config::Config,
    tx: &mpsc::Sender<Result<StatusReport>>,
    repo: String,
    number: u64,
) {
    match clients::github::merge_pull_request(config.github_token.expose_secret(), &repo, number)
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

#[allow(clippy::too_many_arguments)]
fn handle_create_task(
    app: &mut App,
    conn: &rusqlite::Connection,
    config: &config::Config,
    tx: &mpsc::Sender<Result<StatusReport>>,
    title: &str,
    description: Option<&str>,
    kind: domain::TaskKind,
    links: &[String],
    repo: Option<&domain::RepoSlug>,
    origin: &domain::TaskOrigin,
) {
    match store::tasks::active_for_origin(conn, origin) {
        Err(e) => {
            app.ui.flash = Some(format!("Could not check active tasks: {e}"));
        }
        Ok(Some(existing)) => {
            app.ui.flash = Some(format!(
                "{} already active for this signal — close it first",
                existing.id
            ));
        }
        Ok(None) => match workflows::tasks::create(
            title,
            kind,
            description,
            links,
            repo.map(std::string::ToString::to_string).as_deref(),
            origin,
        ) {
            Ok(id) => {
                app.ui.flash = Some(format!("{id} created"));
                request_refresh(app, config, tx, false, None);
            }
            Err(e) => {
                app.ui.flash = Some(format!("Could not create task: {e}"));
            }
        },
    }
}
