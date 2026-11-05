use anyhow::Result;

use super::{Action, App, DetailMode, Effect, Msg, RefreshState, Screen, SubmenuState};
use crate::display::Filter;
use workflows::status::StatusItem;

mod pr;
mod refresh;
mod signals;

pub(crate) use signals::compute_investigate_action;

impl App {
    pub(crate) fn update(&mut self, action: Action) -> Vec<Effect> {
        self.ui.flash = None;
        self.ui.pending_g = false;
        self.clear_submenu_if_stale(action);
        match action {
            Action::Quit => vec![Effect::Quit],
            Action::ToggleHelp => {
                self.ui.show_help = !self.ui.show_help;
                vec![]
            }
            Action::CloseHelp => {
                self.ui.show_help = false;
                vec![]
            }
            Action::Refresh => {
                if matches!(self.data.refresh_state, RefreshState::InProgress) {
                    vec![]
                } else {
                    vec![Effect::StartRefresh]
                }
            }
            Action::Back => self.handle_back(),
            Action::FilterCategory(_)
            | Action::ClearFilter
            | Action::StartQuery
            | Action::AppendQuery(_)
            | Action::BackspaceQuery
            | Action::CommitQuery
            | Action::CancelQuery => self.handle_filter_action(action),
            Action::PendingG => {
                self.ui.pending_g = true;
                vec![]
            }
            Action::ScrollDetailDown | Action::ScrollDetailUp => match &self.ui.screen {
                Screen::UnifiedList { .. } => self.handle_unified_list(action),
                Screen::MergingPr { .. } => vec![],
            },
            Action::PrActionSubmenu
            | Action::CancelPrSubmenu
            | Action::OpenPrDiffInDelta
            | Action::OpenInOcto
            | Action::OpenInLazygit => self.handle_pr_submenu_action(action),
            Action::MoveUp
            | Action::MoveDown
            | Action::MoveToTop
            | Action::MoveToBottom
            | Action::MovePageUp
            | Action::MovePageDown
            | Action::Enter
            | Action::OpenUrl
            | Action::ExpandGroup
            | Action::CollapseGroup
            | Action::ApproveForAgent
            | Action::MergePr
            | Action::CommitMerge
            | Action::CancelMerge
            | Action::Investigate
            | Action::OpenReviewPicker
            | Action::CommitReview(_)
            | Action::CancelReview => match &self.ui.screen {
                Screen::UnifiedList { .. } => self.handle_unified_list(action),
                Screen::MergingPr { .. } => self.handle_merging_pr(action),
            },
        }
    }

    /// Clears the active submenu when the incoming action is not part of that submenu.
    ///
    /// The review picker's own actions are kept so its handler can read the
    /// target it captured; those handlers clear the submenu themselves.
    fn clear_submenu_if_stale(&mut self, action: Action) {
        let belongs_to_active_submenu = match &self.ui.submenu {
            SubmenuState::None => true,
            SubmenuState::PrActions => matches!(
                action,
                Action::PrActionSubmenu
                    | Action::OpenPrDiffInDelta
                    | Action::OpenInLazygit
                    | Action::OpenInOcto
            ),
            SubmenuState::ReviewPicker(_) => matches!(
                action,
                Action::OpenReviewPicker | Action::CommitReview(_) | Action::CancelReview
            ),
        };
        if !belongs_to_active_submenu {
            self.ui.submenu = SubmenuState::None;
        }
    }

    const fn handle_back(&mut self) -> Vec<Effect> {
        // Back collapses a visible detail pane, and does nothing otherwise.
        let is_split_visible = matches!(
            &self.ui.screen,
            Screen::UnifiedList {
                detail_mode: DetailMode::Visible { .. },
                ..
            }
        );
        if is_split_visible {
            if let Screen::UnifiedList { detail_mode, .. } = &mut self.ui.screen {
                *detail_mode = DetailMode::Hidden;
            }
            return vec![];
        }
        vec![]
    }

    fn handle_filter_action(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::FilterCategory(cat) => {
                let new_cat = match &self.ui.screen {
                    Screen::UnifiedList { filter, .. } => {
                        if filter.category == Some(cat) {
                            None
                        } else {
                            Some(cat)
                        }
                    }
                    Screen::MergingPr { .. } => return vec![],
                };
                let query = match &self.ui.screen {
                    Screen::UnifiedList { filter, .. } => filter.query.clone(),
                    Screen::MergingPr { .. } => None,
                };
                self.rebuild_unified(Filter {
                    category: new_cat,
                    query,
                });
                vec![]
            }
            Action::ClearFilter => {
                self.ui.query_input = None;
                let new_filter = match &self.ui.screen {
                    Screen::UnifiedList { filter, .. } => {
                        if filter.query.is_some() {
                            // Peel query first, keep category.
                            Filter {
                                category: filter.category,
                                query: None,
                            }
                        } else {
                            Filter::default()
                        }
                    }
                    Screen::MergingPr { .. } => return vec![],
                };
                self.rebuild_unified(new_filter);
                vec![]
            }
            Action::StartQuery => {
                // Seed from the committed filter so the user can append or trim,
                // rather than forcing them to retype. Esc still clears everything.
                let existing = match &self.ui.screen {
                    Screen::UnifiedList { filter, .. } => filter.query.clone().unwrap_or_default(),
                    Screen::MergingPr { .. } => String::new(),
                };
                self.ui.query_input = Some(existing);
                vec![]
            }
            Action::AppendQuery(c) => {
                if let Some(q) = &mut self.ui.query_input {
                    q.push(c);
                }
                self.sync_query_to_filter();
                vec![]
            }
            Action::BackspaceQuery => {
                if let Some(q) = &mut self.ui.query_input {
                    let _ = q.pop();
                }
                self.sync_query_to_filter();
                vec![]
            }
            Action::CommitQuery => {
                self.ui.query_input = None;
                vec![]
            }
            Action::CancelQuery => {
                self.ui.query_input = None;
                if let Screen::UnifiedList { filter, .. } = &mut self.ui.screen {
                    let cat = filter.category;
                    self.rebuild_unified(Filter {
                        category: cat,
                        query: None,
                    });
                }
                vec![]
            }
            _ => vec![],
        }
    }

    fn handle_pr_submenu_action(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::PrActionSubmenu => {
                self.ui.submenu = SubmenuState::PrActions;
                vec![]
            }
            Action::OpenPrDiffInDelta => {
                self.ui.submenu = SubmenuState::None;
                if let Some(StatusItem::Pr(pr)) = self.ui.screen.selected_status_item() {
                    vec![Effect::OpenPrDiffInDelta {
                        repo: pr.repo.to_string(),
                        number: pr.number,
                        author: pr.author,
                    }]
                } else {
                    vec![]
                }
            }
            Action::OpenInOcto => {
                self.ui.submenu = SubmenuState::None;
                if let Some(StatusItem::Pr(pr)) = self.ui.screen.selected_status_item() {
                    vec![Effect::OpenInOcto {
                        repo: pr.repo.to_string(),
                        number: pr.number,
                        author: pr.author,
                        head_branch: pr.head_branch,
                    }]
                } else {
                    vec![]
                }
            }
            Action::OpenInLazygit => {
                self.ui.submenu = SubmenuState::None;
                if let Some(StatusItem::Pr(pr)) = self.ui.screen.selected_status_item() {
                    vec![Effect::OpenInLazygit {
                        repo: pr.repo.to_string(),
                        number: pr.number,
                        author: pr.author,
                        head_branch: pr.head_branch,
                    }]
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    }
}

pub(crate) fn handle_msg(app: &mut App, msg: Msg) -> Result<Vec<Effect>> {
    match msg {
        Msg::Action(action) => Ok(app.update(action)),
        Msg::Tick => {
            if matches!(app.data.refresh_state, RefreshState::InProgress) {
                Ok(vec![])
            } else {
                Ok(vec![Effect::StartRefresh])
            }
        }
        Msg::FetchResult(Ok(fetched)) => refresh::apply_refresh(app, fetched),
        Msg::FetchResult(Err(e)) => {
            app.data.refresh_state = RefreshState::Failed(e.to_string());
            Ok(vec![])
        }
        Msg::DatabaseRead(read) => {
            refresh::apply_database_read(app, read)?;
            Ok(vec![])
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use rstest::rstest;

    use super::{compute_investigate_action, handle_msg};
    use crate::display::{flatten, Category, DisplayItem, Filter, GroupKey};
    use crate::state::{
        Action, App, DataState, DetailMode, Effect, InvestigateAction, Msg, RefreshState, Screen,
        SubmenuState, UiState,
    };
    use domain::daemon_health::{DaemonHealth, DaemonRecord, LastActivity, Schedule};
    use domain::daemon_pass::PassStatus;
    use domain::known_secrets::KnownSecrets;
    use domain::source_failure::SourceFailure;
    use domain::source_outcomes::SourceOutcomes;
    use workflows::status::{Refresh, StatusItem, StatusReport, SCHEMA_VERSION};

    // --- Building an app ---

    /// An app showing `items` on the unified list, the first selected.
    fn app_on_list(items: Vec<DisplayItem>, filter: Filter, detail_mode: DetailMode) -> App {
        let expanded = HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    flat_rows,
                    items,
                    selected: 0,
                    filter,
                    expanded_groups: expanded,
                    detail_mode,
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    /// An app showing `items` with no filter and the detail pane hidden.
    fn app_with_items(items: Vec<DisplayItem>) -> App {
        app_on_list(items, Filter::default(), DetailMode::Hidden)
    }

    /// An app showing `item` alone, its detail pane open at the top.
    fn split_view_of(item: StatusItem) -> App {
        split_view_scrolled_to(item, 0)
    }

    fn split_view_scrolled_to(item: StatusItem, detail_scroll: u16) -> App {
        app_on_list(
            vec![DisplayItem::Single(item)],
            Filter::default(),
            DetailMode::Visible { detail_scroll },
        )
    }

    fn refreshing() -> App {
        App {
            data: DataState {
                refresh_state: RefreshState::InProgress,
                ..DataState::default()
            },
            ..App::default()
        }
    }

    fn apply(app: &mut App, actions: &[Action]) {
        for action in actions {
            let _ = app.update(*action);
        }
    }

    // --- Reading the screen ---

    fn selected(app: &App) -> usize {
        match app.current_screen() {
            Screen::UnifiedList { selected, .. } => *selected,
            Screen::MergingPr { .. } => panic!("expected UnifiedList"),
        }
    }

    fn detail_mode(app: &App) -> DetailMode {
        match app.current_screen() {
            Screen::UnifiedList { detail_mode, .. } => detail_mode.clone(),
            Screen::MergingPr { .. } => panic!("expected UnifiedList"),
        }
    }

    fn list_len(app: &App) -> usize {
        match app.current_screen() {
            Screen::UnifiedList { items, .. } => items.len(),
            Screen::MergingPr { .. } => panic!("expected UnifiedList"),
        }
    }

    // --- Signals ---

    fn ci_failure() -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("ooloth", "hub"),
            workflow_name: "CI".to_string(),
            job_name: None,
            step_name: None,
            error: None,
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/ooloth/hub/actions/runs/123".to_string(),
        })
    }

    fn gcp_item() -> StatusItem {
        StatusItem::Gcp(domain::GcpEntry {
            title: "errors".to_string(),
            project: "web-app".to_string(),
            repo: domain::RepoSlug::new("owner", "web-app"),
            env: "neuro".to_string(),
            message: domain::UntrustedText::new("something broke"),
            line: domain::UntrustedText::new(r#"{"message":"something broke"}"#),
            lookback: "7d".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://console.cloud.google.com/logs/query".to_string(),
            gcp_project: "web-app-prod-abc123".to_string(),
        })
    }

    fn stub_issue() -> StatusItem {
        StatusItem::Issue(domain::Issue {
            number: 42,
            title: "Test issue".to_string(),
            repo: domain::RepoSlug::new("ooloth", "hub"),
            url: "https://github.com/ooloth/hub/issues/42".to_string(),
            author: "agent".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            labels: vec!["status:needs-human-review".to_string()],
            body: Some("Issue body text.".to_string()),
        })
    }

    fn stub_pr() -> StatusItem {
        StatusItem::Pr(crate::test_support::pull_request())
    }

    #[cfg(feature = "private")]
    fn media_blocked() -> StatusItem {
        StatusItem::MediaBlocked(workflows::private::status::BlockedItem {
            source: "Media".to_string(),
            urgency: domain::Urgency::High,
            age: chrono::Duration::zero(),
            title: domain::UntrustedText::new("Show — S01E01"),
            error: domain::UntrustedText::new("Invalid video file"),
            url: "http://media-server/queue".to_string(),
        })
    }

    // --- Refreshes and reads ---

    fn failure(source: &str) -> SourceFailure {
        SourceFailure::new(
            source,
            &anyhow::anyhow!("unreachable"),
            &KnownSecrets::new(Vec::new()),
        )
    }

    /// A refresh as `workflows::status::run` returns it: the failed sources are the ones `report`
    /// names, and one source answered.
    fn fetched(report: StatusReport) -> Refresh {
        let failures = report.errors.iter().map(|source| failure(source)).collect();
        Refresh {
            report,
            sources: SourceOutcomes::new(vec!["github prs".to_string()], failures),
        }
    }

    /// A refresh that came back with no signals, from the `answered` sources, while `failed` failed.
    fn empty_refresh(answered: &[&str], failed: &[&str]) -> Refresh {
        Refresh {
            report: StatusReport {
                items: vec![],
                errors: failed.iter().map(|source| (*source).to_string()).collect(),
            },
            sources: SourceOutcomes::new(
                answered
                    .iter()
                    .map(|source| (*source).to_string())
                    .collect(),
                failed.iter().map(|source| failure(source)).collect(),
            ),
        }
    }

    fn report_with_ci() -> StatusReport {
        StatusReport {
            items: vec![ci_failure()],
            errors: vec![],
        }
    }

    fn report_with_ci_and_errors() -> StatusReport {
        StatusReport {
            items: vec![ci_failure()],
            errors: vec!["media".to_string(), "linear issues".to_string()],
        }
    }

    fn report_with_two_items() -> StatusReport {
        StatusReport {
            items: vec![ci_failure(), stub_issue()],
            errors: vec![],
        }
    }

    /// An app showing the signals of one earlier refresh that reached its sources.
    fn showing_an_earlier_refresh() -> App {
        let mut app = App::default();
        let _ = handle_msg(&mut app, Msg::FetchResult(Ok(fetched(report_with_ci())))).unwrap();
        app
    }

    /// A read that found `report` cached at `refreshed_at`, at `schema_version`, beside `daemon`.
    fn database_read(
        report: &StatusReport,
        refreshed_at: chrono::DateTime<chrono::Utc>,
        schema_version: i32,
        daemon: DaemonRecord,
    ) -> Msg {
        Msg::DatabaseRead(Ok(store::snapshot::Snapshot {
            cached: Some(store::status_cache::CachedStatus {
                refreshed_at,
                schema_version,
                payload: serde_json::to_string(report).unwrap(),
            }),
            daemon,
        }))
    }

    fn recorded_daemon() -> DaemonRecord {
        DaemonRecord::Recorded(DaemonHealth {
            started_at: chrono::Utc::now(),
            schedule: Schedule::every(std::time::Duration::from_mins(15)),
            activity: LastActivity::Pass(PassStatus::Ok),
        })
    }

    // --- Investigate ---

    #[test]
    fn investigate_action_launches_ci_from_unified_list_selection() {
        let app = app_with_items(vec![DisplayItem::Single(ci_failure())]);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchCi {
                repo: "ooloth/hub".to_string(),
                workflow: "CI".to_string(),
                run_url: "https://github.com/ooloth/hub/actions/runs/123".to_string(),
            }
        );
    }

    #[test]
    fn investigate_action_launches_issue_from_unified_list_selection() {
        let app = app_with_items(vec![DisplayItem::Single(StatusItem::Issue(
            domain::Issue {
                number: 31,
                title: "TUI investigation".to_string(),
                repo: domain::RepoSlug::new("ooloth", "hub"),
                url: "https://github.com/ooloth/hub/issues/31".to_string(),
                author: "agent".to_string(),
                age: chrono::Duration::zero(),
                urgency: domain::Urgency::Low,
                labels: vec![],
                body: None,
            },
        ))]);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchIssue {
                repo: "ooloth/hub".to_string(),
                number: 31,
                author: "agent".to_string(),
            }
        );
    }

    /// `i` on a PR opens a session on that PR whatever state its review is in; the
    /// review picker is the only place a skill is chosen.
    #[rstest]
    #[case::someone_elses_asking_for_my_review(
        domain::PrKind::ToReview,
        "alice",
        false,
        None,
        domain::pr_activity::PrActivity::default()
    )]
    #[case::mine_approved(
        domain::PrKind::Mine,
        "ooloth",
        false,
        Some(domain::ReviewDecision::Approved),
        crate::test_support::verdicts(1, 0)
    )]
    #[case::my_draft_with_changes_requested(
        domain::PrKind::Mine,
        "ooloth",
        true,
        Some(domain::ReviewDecision::ChangesRequested),
        crate::test_support::verdicts(0, 1)
    )]
    fn investigating_a_pr_launches_a_session_on_it_whatever_its_review_state(
        #[case] kind: domain::PrKind,
        #[case] author: &str,
        #[case] draft: bool,
        #[case] review_decision: Option<domain::ReviewDecision>,
        #[case] activity: domain::pr_activity::PrActivity,
    ) {
        let app = app_with_items(vec![DisplayItem::Single(StatusItem::Pr(
            domain::PullRequest {
                kind,
                author: author.to_string(),
                draft,
                review_decision,
                activity,
                ..crate::test_support::pull_request()
            },
        ))]);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchPr {
                repo: "ooloth/hub".to_string(),
                number: 7,
                kind,
                author: author.to_string(),
                head_branch: "feat/thing".to_string(),
                base_branch: "main".to_string(),
            }
        );
    }

    #[cfg(feature = "private")]
    #[test]
    fn investigate_action_launches_media_from_unified_list_selection() {
        let app = app_with_items(vec![DisplayItem::Single(media_blocked())]);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchMediaBlocked {
                title: domain::UntrustedText::new("Show — S01E01"),
                error: domain::UntrustedText::new("Invalid video file"),
            }
        );
    }

    // --- Help, flash, quit ---

    #[test]
    fn update_toggle_help_flips_flag() {
        let mut app = App::default();
        let _ = app.update(Action::ToggleHelp);
        assert!(app.ui.show_help);
        let _ = app.update(Action::ToggleHelp);
        assert!(!app.ui.show_help);
    }

    #[test]
    fn update_clears_flash_before_applying_action() {
        let mut app = App {
            ui: UiState {
                flash: Some("stale".to_string()),
                ..UiState::default()
            },
            ..App::default()
        };
        let _ = app.update(Action::ToggleHelp);
        assert!(app.ui.flash.is_none());
    }

    #[test]
    fn update_quit_returns_quit_effect() {
        let mut app = App::default();
        assert!(matches!(
            app.update(Action::Quit).as_slice(),
            [Effect::Quit]
        ));
    }

    #[test]
    fn handle_msg_action_delegates_to_update() {
        let mut app = App::default();
        let effects = handle_msg(&mut app, Msg::Action(Action::Quit)).unwrap();
        assert!(matches!(effects.as_slice(), [Effect::Quit]));
    }

    // --- Groups ---

    fn a_group_of_two(item: fn() -> StatusItem) -> App {
        app_with_items(vec![DisplayItem::Group {
            label: GroupKey::new("hub".to_string()),
            items: vec![item(), item()],
        }])
    }

    #[test]
    fn enter_on_ci_group_header_is_noop() {
        let mut app = a_group_of_two(ci_failure);
        let _ = app.update(Action::Enter);
        assert_eq!(app.active_list_len(), 1);
    }

    #[test]
    fn enter_on_gcp_group_header_activates_split_view() {
        let mut app = a_group_of_two(gcp_item);
        let _ = app.update(Action::Enter);
        assert_eq!(detail_mode(&app), DetailMode::Visible { detail_scroll: 0 });
    }

    #[test]
    fn expand_group_expands_collapsed_header() {
        let mut app = a_group_of_two(ci_failure);
        assert_eq!(app.active_list_len(), 1);
        let _ = app.update(Action::ExpandGroup);
        assert_eq!(app.active_list_len(), 3);
    }

    #[test]
    fn expand_group_on_expanded_header_is_noop() {
        let mut app = a_group_of_two(ci_failure);
        let _ = app.update(Action::ExpandGroup);
        assert_eq!(app.active_list_len(), 3);
        let _ = app.update(Action::ExpandGroup);
        assert_eq!(app.active_list_len(), 3);
    }

    #[test]
    fn collapse_group_collapses_expanded_header() {
        let mut app = a_group_of_two(ci_failure);
        let _ = app.update(Action::ExpandGroup);
        assert_eq!(app.active_list_len(), 3);
        let _ = app.update(Action::CollapseGroup);
        assert_eq!(app.active_list_len(), 1);
    }

    #[test]
    fn collapse_group_on_child_jumps_to_header() {
        let mut app = a_group_of_two(ci_failure);
        apply(&mut app, &[Action::ExpandGroup, Action::MoveDown]);
        assert_eq!(selected(&app), 1);
        let _ = app.update(Action::CollapseGroup);
        assert_eq!(selected(&app), 0);
        assert_eq!(app.active_list_len(), 1);
    }

    // --- Refresh and tick ---

    #[rstest]
    #[case::the_refresh_key(Msg::Action(Action::Refresh))]
    #[case::a_tick(Msg::Tick)]
    fn a_refresh_starts_when_none_is_running(#[case] msg: Msg) {
        let mut app = App::default();
        let effects = handle_msg(&mut app, msg).unwrap();
        assert!(matches!(effects.as_slice(), [Effect::StartRefresh]));
    }

    #[rstest]
    #[case::the_refresh_key(Msg::Action(Action::Refresh))]
    #[case::a_tick(Msg::Tick)]
    fn no_second_refresh_starts_while_one_is_running(#[case] msg: Msg) {
        let mut app = refreshing();
        let effects = handle_msg(&mut app, msg).unwrap();
        assert!(matches!(app.data.refresh_state, RefreshState::InProgress));
        assert!(effects.is_empty());
    }

    #[test]
    fn handle_msg_fetch_ok_sets_idle_and_populates_items() {
        let mut app = refreshing();
        let _ = handle_msg(&mut app, Msg::FetchResult(Ok(fetched(report_with_ci())))).unwrap();
        assert!(matches!(app.data.refresh_state, RefreshState::Idle));
        assert_eq!(list_len(&app), 1);
        assert!(app.data.last_updated.is_some());
    }

    #[test]
    fn handle_msg_fetch_ok_returns_write_cache_effect() {
        let mut app = App::default();
        let effects =
            handle_msg(&mut app, Msg::FetchResult(Ok(fetched(report_with_ci())))).unwrap();
        assert!(matches!(effects.as_slice(), [Effect::WriteCache(_)]));
    }

    #[rstest]
    #[case::every_source_failed(&[], &["github prs", "github issues"])]
    #[case::one_failed_and_the_rest_found_nothing(&["github issues"], &["github prs"])]
    fn a_refresh_with_no_signals_and_a_failure_does_not_write_the_cache(
        #[case] answered: &[&str],
        #[case] failed: &[&str],
    ) {
        let mut app = showing_an_earlier_refresh();

        let effects = handle_msg(
            &mut app,
            Msg::FetchResult(Ok(empty_refresh(answered, failed))),
        )
        .unwrap();

        assert!(effects.is_empty(), "expected no cache write");
    }

    #[test]
    fn a_refused_refresh_keeps_showing_the_earlier_signals() {
        let mut app = showing_an_earlier_refresh();

        let _ = handle_msg(
            &mut app,
            Msg::FetchResult(Ok(empty_refresh(&[], &["github prs"]))),
        )
        .unwrap();

        assert_eq!(list_len(&app), 1);
        assert_eq!(app.data.raw_items.len(), 1);
    }

    #[test]
    fn a_refused_refresh_keeps_the_earlier_updated_time() {
        let mut app = showing_an_earlier_refresh();
        let earlier = app.data.last_updated;

        let _ = handle_msg(
            &mut app,
            Msg::FetchResult(Ok(empty_refresh(&[], &["github prs"]))),
        )
        .unwrap();

        assert!(earlier.is_some());
        assert_eq!(app.data.last_updated, earlier);
    }

    #[test]
    fn a_refused_refresh_names_the_sources_that_failed() {
        let mut app = showing_an_earlier_refresh();

        let _ = handle_msg(
            &mut app,
            Msg::FetchResult(Ok(empty_refresh(&[], &["github prs", "github issues"]))),
        )
        .unwrap();

        assert!(
            matches!(&app.data.refresh_state, RefreshState::Partial(sources) if sources == &["github prs", "github issues"]),
            "got {:?}",
            app.data.refresh_state
        );
    }

    #[test]
    fn a_refresh_that_found_the_queue_empty_with_nothing_failing_writes_the_cache_and_empties_the_list(
    ) {
        let mut app = showing_an_earlier_refresh();

        let effects = handle_msg(
            &mut app,
            Msg::FetchResult(Ok(empty_refresh(&["github prs"], &[]))),
        )
        .unwrap();

        assert!(matches!(effects.as_slice(), [Effect::WriteCache(_)]));
        assert_eq!(list_len(&app), 0);
    }

    #[test]
    fn handle_msg_fetch_ok_with_errors_sets_partial_state() {
        let mut app = refreshing();
        let _ = handle_msg(
            &mut app,
            Msg::FetchResult(Ok(fetched(report_with_ci_and_errors()))),
        )
        .unwrap();
        assert!(
            matches!(&app.data.refresh_state, RefreshState::Partial(sources) if sources == &["media", "linear issues"])
        );
    }

    #[test]
    fn handle_msg_fetch_ok_with_errors_still_populates_items() {
        let mut app = App::default();
        let _ = handle_msg(
            &mut app,
            Msg::FetchResult(Ok(fetched(report_with_ci_and_errors()))),
        )
        .unwrap();
        assert_eq!(list_len(&app), 1);
    }

    #[test]
    fn handle_msg_fetch_ok_with_errors_returns_write_cache_effect() {
        let mut app = App::default();
        let effects = handle_msg(
            &mut app,
            Msg::FetchResult(Ok(fetched(report_with_ci_and_errors()))),
        )
        .unwrap();
        assert!(matches!(effects.as_slice(), [Effect::WriteCache(_)]));
    }

    #[test]
    fn handle_msg_fetch_err_sets_failed_state() {
        let mut app = refreshing();
        let effects = handle_msg(
            &mut app,
            Msg::FetchResult(Err(anyhow::anyhow!("network error"))),
        )
        .unwrap();
        assert!(matches!(app.data.refresh_state, RefreshState::Failed(_)));
        assert!(effects.is_empty());
    }

    #[test]
    fn refresh_in_unified_list_preserves_selection_when_list_unchanged() {
        let mut app = app_with_items(vec![
            DisplayItem::Single(ci_failure()),
            DisplayItem::Single(stub_issue()),
        ]);
        apply(&mut app, &[Action::MoveDown]);
        let _ = handle_msg(
            &mut app,
            Msg::FetchResult(Ok(fetched(report_with_two_items()))),
        )
        .unwrap();
        assert_eq!(selected(&app), 1);
    }

    #[test]
    fn refresh_in_unified_list_clamps_selection_when_list_shrinks() {
        let mut app = app_with_items(vec![
            DisplayItem::Single(ci_failure()),
            DisplayItem::Single(ci_failure()),
            DisplayItem::Single(ci_failure()),
            DisplayItem::Single(stub_issue()),
        ]);
        // The cursor is on row 3; the refresh brings two rows, so it clamps to 1.
        apply(&mut app, &[Action::MoveToBottom]);
        let _ = handle_msg(
            &mut app,
            Msg::FetchResult(Ok(fetched(report_with_two_items()))),
        )
        .unwrap();
        assert_eq!(selected(&app), 1);
    }

    #[test]
    fn refresh_in_unified_list_applies_current_filter() {
        let mut app = app_on_list(
            vec![],
            Filter {
                category: Some(Category::Issues),
                query: None,
            },
            DetailMode::Hidden,
        );
        let _ = handle_msg(
            &mut app,
            Msg::FetchResult(Ok(fetched(report_with_two_items()))),
        )
        .unwrap();
        let Screen::UnifiedList { items, .. } = app.current_screen() else {
            panic!("expected UnifiedList");
        };
        assert_eq!(items.len(), 1);
        assert!(matches!(
            &items[0],
            DisplayItem::Single(StatusItem::Issue(_))
        ));
    }

    // --- Database reads ---

    #[test]
    fn a_read_with_newer_signals_shows_them_beside_the_daemons_record() {
        let mut app = App::default();
        let cached_at = chrono::Utc::now() - chrono::Duration::minutes(5);

        let effects = handle_msg(
            &mut app,
            database_read(
                &report_with_ci(),
                cached_at,
                SCHEMA_VERSION,
                recorded_daemon(),
            ),
        )
        .unwrap();

        assert!(effects.is_empty(), "a read writes nothing back");
        assert_eq!(list_len(&app), 1);
        assert_eq!(app.data.last_updated, Some(cached_at));
        assert!(matches!(app.data.refresh_state, RefreshState::Idle));
        assert!(matches!(app.data.daemon, DaemonRecord::Recorded(_)));
    }

    #[test]
    fn a_read_with_failed_sources_in_its_signals_names_them() {
        let mut app = App::default();

        let _ = handle_msg(
            &mut app,
            database_read(
                &report_with_ci_and_errors(),
                chrono::Utc::now(),
                SCHEMA_VERSION,
                recorded_daemon(),
            ),
        )
        .unwrap();

        assert!(
            matches!(&app.data.refresh_state, RefreshState::Partial(sources) if sources == &["media", "linear issues"])
        );
    }

    #[rstest]
    #[case::the_same_signals(chrono::Duration::zero())]
    #[case::older_signals(chrono::Duration::minutes(-10))]
    fn a_read_with_no_newer_signals_leaves_the_list_but_takes_the_record(
        #[case] cached_relative_to_shown: chrono::Duration,
    ) {
        let shown_at = chrono::Utc::now() - chrono::Duration::minutes(5);
        let mut app = App {
            data: DataState {
                last_updated: Some(shown_at),
                ..DataState::default()
            },
            ..App::default()
        };

        let _ = handle_msg(
            &mut app,
            database_read(
                &report_with_ci(),
                shown_at + cached_relative_to_shown,
                SCHEMA_VERSION,
                recorded_daemon(),
            ),
        )
        .unwrap();

        assert_eq!(list_len(&app), 0);
        assert_eq!(app.data.last_updated, Some(shown_at));
        assert!(matches!(app.data.daemon, DaemonRecord::Recorded(_)));
    }

    #[test]
    fn signals_of_another_schema_version_are_left_unread() {
        let mut app = App::default();

        let _ = handle_msg(
            &mut app,
            database_read(
                &report_with_ci(),
                chrono::Utc::now(),
                SCHEMA_VERSION - 1,
                recorded_daemon(),
            ),
        )
        .unwrap();

        assert_eq!(list_len(&app), 0);
        assert_eq!(app.data.last_updated, None);
    }

    #[test]
    fn newer_signals_during_the_tuis_own_refresh_leave_it_refreshing() {
        let mut app = refreshing();

        let _ = handle_msg(
            &mut app,
            database_read(
                &report_with_ci_and_errors(),
                chrono::Utc::now(),
                SCHEMA_VERSION,
                recorded_daemon(),
            ),
        )
        .unwrap();

        assert_eq!(list_len(&app), 1);
        assert!(matches!(app.data.refresh_state, RefreshState::InProgress));
    }

    #[test]
    fn a_failed_read_leaves_the_signals_and_keeps_the_reason_until_a_read_succeeds() {
        let shown_at = chrono::Utc::now() - chrono::Duration::minutes(5);
        let mut app = App::default();
        let _ = handle_msg(
            &mut app,
            database_read(
                &report_with_ci(),
                shown_at,
                SCHEMA_VERSION,
                recorded_daemon(),
            ),
        )
        .unwrap();

        let _ = handle_msg(
            &mut app,
            Msg::DatabaseRead(Err("disk I/O error".to_string())),
        )
        .unwrap();

        assert_eq!(list_len(&app), 1);
        assert_eq!(app.data.last_updated, Some(shown_at));
        assert!(matches!(app.data.daemon, DaemonRecord::Recorded(_)));
        assert_eq!(app.data.database_error.as_deref(), Some("disk I/O error"));

        let _ = handle_msg(
            &mut app,
            database_read(
                &report_with_ci(),
                shown_at,
                SCHEMA_VERSION,
                recorded_daemon(),
            ),
        )
        .unwrap();

        assert_eq!(app.data.database_error, None);
    }

    // --- Query mode re-entry ---

    #[test]
    fn start_query_with_no_prior_filter_begins_empty() {
        let mut app = App::default();
        let _ = app.update(Action::StartQuery);
        assert_eq!(app.ui.query_input.as_deref(), Some(""));
    }

    #[test]
    fn start_query_after_committed_filter_seeds_input_from_filter() {
        let mut app = App::default();
        // Type "fo" and commit it so filter.query becomes Some("fo").
        apply(
            &mut app,
            &[
                Action::StartQuery,
                Action::AppendQuery('f'),
                Action::AppendQuery('o'),
                Action::CommitQuery,
            ],
        );
        // Re-entering query mode should restore the committed text, not start blank.
        let _ = app.update(Action::StartQuery);
        assert_eq!(app.ui.query_input.as_deref(), Some("fo"));
    }

    #[test]
    fn appending_after_query_restart_extends_committed_text() {
        let mut app = App::default();
        // Commit "fo", re-enter, then append 'o' → input becomes "foo".
        apply(
            &mut app,
            &[
                Action::StartQuery,
                Action::AppendQuery('f'),
                Action::AppendQuery('o'),
                Action::CommitQuery,
                Action::StartQuery,
                Action::AppendQuery('o'),
            ],
        );
        assert_eq!(app.ui.query_input.as_deref(), Some("foo"));
    }

    #[test]
    fn backspace_after_query_restart_trims_from_end_of_committed_text() {
        let mut app = App::default();
        // Commit "foo", re-enter, then Backspace → input becomes "fo".
        apply(
            &mut app,
            &[
                Action::StartQuery,
                Action::AppendQuery('f'),
                Action::AppendQuery('o'),
                Action::AppendQuery('o'),
                Action::CommitQuery,
                Action::StartQuery,
                Action::BackspaceQuery,
            ],
        );
        assert_eq!(app.ui.query_input.as_deref(), Some("fo"));
    }

    // --- Vim navigation ---

    fn items_n(n: usize) -> Vec<DisplayItem> {
        (0..n).map(|_| DisplayItem::Single(ci_failure())).collect()
    }

    #[test]
    fn pending_g_sets_pending_flag() {
        let mut app = App::default();
        let _ = app.update(Action::PendingG);
        assert!(app.ui.pending_g);
    }

    #[test]
    fn any_subsequent_action_clears_pending_g() {
        let mut app = app_with_items(items_n(5));
        apply(&mut app, &[Action::PendingG, Action::MoveDown]);
        assert!(!app.ui.pending_g);
    }

    #[test]
    fn move_to_top_goes_to_first_item() {
        let mut app = app_with_items(items_n(5));
        apply(&mut app, &[Action::MoveDown, Action::MoveDown]);
        assert_eq!(selected(&app), 2);
        let _ = app.update(Action::MoveToTop);
        assert_eq!(selected(&app), 0);
    }

    #[test]
    fn move_to_bottom_goes_to_last_item() {
        let mut app = app_with_items(items_n(5));
        let _ = app.update(Action::MoveToBottom);
        assert_eq!(selected(&app), 4);
    }

    #[test]
    fn move_to_bottom_on_empty_list_is_noop() {
        let mut app = app_with_items(vec![]);
        let _ = app.update(Action::MoveToBottom);
        assert_eq!(selected(&app), 0);
    }

    #[test]
    fn move_page_down_advances_by_10() {
        let mut app = app_with_items(items_n(25));
        let _ = app.update(Action::MovePageDown);
        assert_eq!(selected(&app), 10);
    }

    #[test]
    fn move_page_down_clamps_at_last_item() {
        let mut app = app_with_items(items_n(5));
        let _ = app.update(Action::MovePageDown);
        assert_eq!(selected(&app), 4);
    }

    #[test]
    fn move_page_up_retreats_by_10() {
        let mut app = app_with_items(items_n(25));
        apply(&mut app, &[Action::MovePageDown, Action::MovePageDown]);
        assert_eq!(selected(&app), 20);
        let _ = app.update(Action::MovePageUp);
        assert_eq!(selected(&app), 10);
    }

    #[test]
    fn move_page_up_clamps_at_first_item() {
        let mut app = app_with_items(items_n(5));
        apply(&mut app, &[Action::MoveDown, Action::MoveDown]);
        let _ = app.update(Action::MovePageUp);
        assert_eq!(selected(&app), 0);
    }

    // --- The detail pane ---

    #[rstest]
    #[case::an_issue(stub_issue())]
    #[case::a_ci_failure(ci_failure())]
    fn enter_on_an_item_opens_its_detail_pane_at_the_top(#[case] item: StatusItem) {
        let mut app = app_with_items(vec![DisplayItem::Single(item)]);
        let _ = app.update(Action::Enter);
        assert_eq!(detail_mode(&app), DetailMode::Visible { detail_scroll: 0 });
    }

    #[test]
    fn enter_while_split_view_active_is_noop() {
        let mut app = split_view_scrolled_to(ci_failure(), 3);
        let _ = app.update(Action::Enter);
        assert_eq!(detail_mode(&app), DetailMode::Visible { detail_scroll: 3 });
    }

    #[test]
    fn back_from_split_view_collapses_to_fullscreen_list() {
        let mut app = split_view_scrolled_to(ci_failure(), 2);
        let _ = app.update(Action::Back);
        assert_eq!(detail_mode(&app), DetailMode::Hidden);
    }

    #[test]
    fn back_with_the_detail_pane_hidden_does_nothing() {
        let mut app = app_with_items(vec![]);
        app.data.raw_items = vec![stub_issue(), ci_failure(), gcp_item(), ci_failure()];
        apply(
            &mut app,
            &[
                Action::FilterCategory(Category::Errors),
                Action::MoveDown,
                Action::MoveDown,
            ],
        );

        let effects = app.update(Action::Back);

        assert!(effects.is_empty());
        assert_eq!(detail_mode(&app), DetailMode::Hidden);
        assert_eq!(selected(&app), 2);
        let Screen::UnifiedList { filter, .. } = app.current_screen() else {
            panic!("expected UnifiedList");
        };
        assert_eq!(filter.category, Some(Category::Errors));
    }

    #[test]
    fn back_from_issue_detail_restores_selection() {
        let mut app = app_with_items(vec![
            DisplayItem::Single(ci_failure()),
            DisplayItem::Single(stub_issue()),
        ]);
        apply(&mut app, &[Action::MoveDown, Action::Enter, Action::Back]);
        assert_eq!(selected(&app), 1);
    }

    #[test]
    fn clear_filter_while_hidden_clears_filter_and_leaves_detail_hidden() {
        let mut app = app_on_list(
            vec![DisplayItem::Single(ci_failure())],
            Filter {
                category: Some(Category::Prs),
                query: None,
            },
            DetailMode::Hidden,
        );
        let _ = app.update(Action::ClearFilter);
        match app.current_screen() {
            Screen::UnifiedList {
                filter,
                detail_mode,
                ..
            } => {
                assert!(filter.is_empty());
                assert_eq!(detail_mode, &DetailMode::Hidden);
            }
            Screen::MergingPr { .. } => panic!("expected UnifiedList"),
        }
    }

    #[test]
    fn move_down_while_split_view_active_resets_detail_scroll() {
        let mut app = app_on_list(
            items_n(2),
            Filter::default(),
            DetailMode::Visible { detail_scroll: 7 },
        );
        let _ = app.update(Action::MoveDown);
        assert_eq!(selected(&app), 1);
        assert_eq!(detail_mode(&app), DetailMode::Visible { detail_scroll: 0 });
    }

    #[test]
    fn scroll_detail_down_while_split_view_active_increments_scroll() {
        let mut app = split_view_scrolled_to(ci_failure(), 0);
        let _ = app.update(Action::ScrollDetailDown);
        assert_eq!(detail_mode(&app), DetailMode::Visible { detail_scroll: 1 });
    }

    #[test]
    fn scroll_detail_down_while_hidden_is_noop() {
        let mut app = app_with_items(vec![DisplayItem::Single(ci_failure())]);
        let _ = app.update(Action::ScrollDetailDown);
        assert_eq!(detail_mode(&app), DetailMode::Hidden);
    }

    /// Scrolling up saturates at 0, so it decrements from anywhere above it.
    #[test]
    fn scroll_detail_up_while_split_view_active_decrements_scroll() {
        let mut app = split_view_scrolled_to(ci_failure(), 3);
        let _ = app.update(Action::ScrollDetailUp);
        assert_eq!(detail_mode(&app), DetailMode::Visible { detail_scroll: 2 });
    }

    // --- ApproveForAgent from split view ---

    #[test]
    fn approve_for_agent_from_split_view_emits_set_issue_labels() {
        let mut app = split_view_of(stub_issue());
        let effects = app.update(Action::ApproveForAgent);
        assert_eq!(effects.len(), 1);
        let Effect::SetIssueLabels {
            repo,
            number,
            labels,
        } = effects.into_iter().next().unwrap()
        else {
            panic!("expected SetIssueLabels");
        };
        assert_eq!(repo, "ooloth/hub");
        assert_eq!(number, 42);
        assert!(labels.contains(&"status:ready-for-agent".to_string()));
        assert!(!labels.contains(&"status:needs-human-review".to_string()));
    }

    // --- MergePr flow ---

    fn app_in_merging() -> App {
        let mut app = split_view_of(stub_pr());
        let _ = app.update(Action::MergePr);
        app
    }

    #[test]
    fn merge_pr_from_split_view_transitions_to_merging_screen() {
        let app = app_in_merging();
        assert!(matches!(app.current_screen(), Screen::MergingPr { .. }));
    }

    #[test]
    fn merge_pr_preserves_pr() {
        let app = app_in_merging();
        let Screen::MergingPr { pr, .. } = app.current_screen() else {
            panic!("expected MergingPr");
        };
        assert_eq!(pr.number, 7);
        assert_eq!(pr.repo.to_string(), "ooloth/hub");
    }

    #[test]
    fn cancel_merge_returns_to_unified_list() {
        let mut app = app_in_merging();
        let _ = app.update(Action::CancelMerge);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    #[test]
    fn cancel_merge_emits_no_effects() {
        let mut app = app_in_merging();
        let effects = app.update(Action::CancelMerge);
        assert!(effects.is_empty());
    }

    #[test]
    fn commit_merge_returns_to_unified_list() {
        let mut app = app_in_merging();
        let _ = app.update(Action::CommitMerge);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    #[test]
    fn commit_merge_emits_merge_pull_request_effect_with_correct_fields() {
        let mut app = app_in_merging();
        let effects = app.update(Action::CommitMerge);
        assert_eq!(effects.len(), 1);
        let Effect::MergePullRequest { repo, number } = effects.into_iter().next().unwrap() else {
            panic!("expected MergePullRequest");
        };
        assert_eq!(repo, "ooloth/hub");
        assert_eq!(number, 7);
    }

    #[test]
    fn refresh_in_merging_preserves_screen_variant() {
        let mut app = app_in_merging();
        let _ = handle_msg(&mut app, Msg::FetchResult(Ok(fetched(report_with_ci())))).unwrap();
        assert!(matches!(app.current_screen(), Screen::MergingPr { .. }));
    }

    #[test]
    fn refresh_in_merging_updates_parent_items() {
        let mut app = app_in_merging();
        let _ = handle_msg(&mut app, Msg::FetchResult(Ok(fetched(report_with_ci())))).unwrap();
        let Screen::MergingPr { parent, .. } = app.current_screen() else {
            panic!("expected MergingPr");
        };
        assert!(matches!(
            parent.items[0],
            DisplayItem::Single(StatusItem::Ci(_))
        ));
    }

    // --- OpenInOcto / OpenInLazygit from split view ---

    fn opens_the_pr_in_octo(effect: &Effect) -> bool {
        matches!(effect, Effect::OpenInOcto { repo, number: 7, author, head_branch }
            if repo == "ooloth/hub" && author == "ooloth" && head_branch == "feat/thing")
    }

    fn opens_the_pr_in_lazygit(effect: &Effect) -> bool {
        matches!(effect, Effect::OpenInLazygit { repo, number: 7, author, head_branch }
            if repo == "ooloth/hub" && author == "ooloth" && head_branch == "feat/thing")
    }

    fn opens_the_pr_diff(effect: &Effect) -> bool {
        matches!(effect, Effect::OpenPrDiffInDelta { repo, number: 7, author }
            if repo == "ooloth/hub" && author == "ooloth")
    }

    #[rstest]
    #[case::diff(Action::OpenPrDiffInDelta, opens_the_pr_diff)]
    #[case::octo(Action::OpenInOcto, opens_the_pr_in_octo)]
    #[case::lazygit(Action::OpenInLazygit, opens_the_pr_in_lazygit)]
    fn opening_the_pr_from_its_submenu_emits_the_effect_with_its_identity(
        #[case] action: Action,
        #[case] expected: fn(&Effect) -> bool,
    ) {
        let mut app = split_view_of(stub_pr());
        let _ = app.update(Action::PrActionSubmenu);
        let effects = app.update(action);
        assert!(
            matches!(effects.as_slice(), [effect] if expected(effect)),
            "got {} effects",
            effects.len()
        );
    }

    // --- OpenReviewPicker from split view ---

    fn armed_picker(app: &mut App) -> crate::state::PrReviewTarget {
        let _ = app.update(Action::OpenReviewPicker);
        let SubmenuState::ReviewPicker(target) = &app.ui.submenu else {
            panic!("expected an armed review picker");
        };
        target.clone()
    }

    #[test]
    fn open_review_picker_from_split_view_captures_the_pr_and_stays_on_screen() {
        let mut app = split_view_of(stub_pr());
        let target = armed_picker(&mut app);
        assert_eq!(target.repo, "ooloth/hub");
        assert_eq!(target.number, 7);
        assert_eq!(target.head_branch, "feat/thing");
        assert_eq!(target.ownership, crate::state::PrOwnership::Mine);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    /// A PR by `alice`, who is not the repository's owner, so a login taken from the repository
    /// or the user instead of the PR's author fails.
    fn pr_by_alice() -> StatusItem {
        StatusItem::Pr(domain::PullRequest {
            author: "alice".to_string(),
            ..crate::test_support::pull_request()
        })
    }

    #[test]
    fn the_review_picker_carries_the_prs_author_login() {
        let mut app = split_view_of(pr_by_alice());
        assert_eq!(armed_picker(&mut app).author, "alice");
    }

    #[test]
    fn investigating_a_pr_launches_with_its_authors_login() {
        let mut app = app_with_items(vec![DisplayItem::Single(pr_by_alice())]);
        let effects = app.update(Action::Investigate);
        let [Effect::LaunchPr { author, .. }] = effects.as_slice() else {
            panic!("expected one LaunchPr");
        };
        assert_eq!(author, "alice");
    }

    #[test]
    fn investigating_an_issue_launches_with_its_authors_login() {
        let mut app = app_with_items(vec![DisplayItem::Single(stub_issue())]);
        let effects = app.update(Action::Investigate);
        let [Effect::LaunchIssue { author, .. }] = effects.as_slice() else {
            panic!("expected one LaunchIssue");
        };
        assert_eq!(author, "agent");
    }

    #[test]
    fn open_review_picker_on_a_non_pr_row_leaves_the_submenu_closed() {
        let mut app = app_with_items(vec![DisplayItem::Single(ci_failure())]);
        let _ = app.update(Action::OpenReviewPicker);
        assert_eq!(app.ui.submenu, SubmenuState::None);
    }

    #[rstest]
    #[case::review_mine(crate::state::PrReview::ReviewMine)]
    #[case::fix_mine(crate::state::PrReview::FixMine)]
    #[case::answer_reviewers(crate::state::PrReview::AnswerReviewers)]
    fn committing_a_review_emits_the_effect_for_the_captured_pr(
        #[case] review: crate::state::PrReview,
    ) {
        let mut app = split_view_of(stub_pr());
        let target = armed_picker(&mut app);
        let effects = app.update(Action::CommitReview(review));
        assert_eq!(app.ui.submenu, SubmenuState::None);
        assert_eq!(effects.len(), 1);
        let Effect::ReviewPr {
            target: launched,
            review: launched_review,
        } = effects.into_iter().next().unwrap()
        else {
            panic!("expected ReviewPr");
        };
        assert_eq!(launched, target);
        assert_eq!(launched_review, review);
    }

    #[test]
    fn a_refresh_while_the_picker_is_armed_does_not_redirect_the_launch() {
        let mut app = split_view_of(stub_pr());
        let target = armed_picker(&mut app);

        // A background refresh replaces the list with a different PR under the
        // same selection index. The captured target must survive it.
        let _ = handle_msg(&mut app, Msg::FetchResult(Ok(fetched(report_with_ci())))).unwrap();

        let effects = app.update(Action::CommitReview(crate::state::PrReview::FixMine));
        assert_eq!(effects.len(), 1);
        let Effect::ReviewPr {
            target: launched, ..
        } = effects.into_iter().next().unwrap()
        else {
            panic!("expected ReviewPr");
        };
        assert_eq!(launched, target);
    }

    #[test]
    fn committing_a_review_with_no_armed_picker_emits_nothing() {
        let mut app = split_view_of(stub_pr());
        let effects = app.update(Action::CommitReview(crate::state::PrReview::FixMine));
        assert!(effects.is_empty());
    }

    #[test]
    fn cancel_review_from_split_view_clears_submenu_stays_on_screen() {
        let mut app = split_view_of(stub_pr());
        let _ = armed_picker(&mut app);
        let effects = app.update(Action::CancelReview);
        assert_eq!(app.ui.submenu, SubmenuState::None);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
        assert!(effects.is_empty());
    }

    #[test]
    fn an_unrelated_action_clears_an_armed_review_picker() {
        let mut app = split_view_of(stub_pr());
        let _ = armed_picker(&mut app);
        let _ = app.update(Action::MoveDown);
        assert_eq!(app.ui.submenu, SubmenuState::None);
    }

    // --- OpenUrl ---

    #[test]
    fn open_url_on_item_with_url_emits_open_url_effect() {
        let mut app = app_with_items(vec![DisplayItem::Single(stub_pr())]);
        let effects = app.update(Action::OpenUrl);
        assert!(matches!(
            effects.as_slice(),
            [Effect::OpenUrl(u)] if u == "https://github.com/ooloth/hub/pull/7"
        ));
    }

    #[test]
    fn open_url_with_no_selection_is_noop() {
        let mut app = app_with_items(vec![]);
        let effects = app.update(Action::OpenUrl);
        assert!(effects.is_empty());
    }
}
