use anyhow::{Context, Result};
use chrono::Utc;
use ratatui::widgets::ListState;

use super::{
    Action, App, DetailView, Effect, EnterAction, InvestigateAction, Msg, RefreshState, Screen,
};
use crate::display::{
    build_unified, item_investigation, item_url, DisplayItem, Filter, InvestigationKind,
    ListSnapshot,
};

impl App {
    pub(crate) fn update(&mut self, action: Action) -> Vec<Effect> {
        self.ui.flash = None;
        self.ui.pending_g = false;
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
                if !matches!(self.data.refresh_state, RefreshState::InProgress) {
                    self.data.refresh_state = RefreshState::InProgress;
                    vec![Effect::StartRefresh]
                } else {
                    vec![]
                }
            }
            Action::Back => {
                self.ui.screen = match std::mem::take(&mut self.ui.screen) {
                    Screen::Detail { parent, .. } => Screen::UnifiedList {
                        items: parent.items,
                        selected: parent.selected,
                        filter: parent.filter,
                    },
                    // Already at top level — no-op.
                    other => other,
                };
                vec![]
            }
            Action::FilterCategory(cat) => {
                let new_cat = match &self.ui.screen {
                    Screen::UnifiedList { filter, .. } => {
                        if filter.category == Some(cat) {
                            None
                        } else {
                            Some(cat)
                        }
                    }
                    _ => return vec![],
                };
                let query = match &self.ui.screen {
                    Screen::UnifiedList { filter, .. } => filter.query.clone(),
                    _ => None,
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
                    _ => return vec![],
                };
                self.rebuild_unified(new_filter);
                vec![]
            }
            Action::StartQuery => {
                // Seed from the committed filter so the user can append or trim,
                // rather than forcing them to retype. Esc still clears everything.
                let existing = match &self.ui.screen {
                    Screen::UnifiedList { filter, .. } => filter.query.clone().unwrap_or_default(),
                    _ => String::new(),
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
                    q.pop();
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
                let cat = match &self.ui.screen {
                    Screen::UnifiedList { filter, .. } => filter.category,
                    _ => return vec![],
                };
                self.rebuild_unified(Filter {
                    category: cat,
                    query: None,
                });
                vec![]
            }
            Action::PendingG => {
                self.ui.pending_g = true;
                vec![]
            }
            Action::MoveUp
            | Action::MoveDown
            | Action::MoveToTop
            | Action::MoveToBottom
            | Action::MovePageUp
            | Action::MovePageDown
            | Action::Enter
            | Action::Investigate => {
                if matches!(self.ui.screen, Screen::UnifiedList { .. }) {
                    self.handle_unified_list(action)
                } else {
                    self.handle_detail(action)
                }
            }
        }
    }

    fn rebuild_unified(&mut self, new_filter: Filter) {
        let items = build_unified(self.data.raw_items.clone(), &new_filter);
        if let Screen::UnifiedList {
            items: ref mut i,
            selected: ref mut s,
            filter: ref mut f,
        } = self.ui.screen
        {
            *i = items;
            *s = 0;
            *f = new_filter;
        }
    }

    fn sync_query_to_filter(&mut self) {
        let query_text = self.ui.query_input.clone().filter(|q| !q.is_empty());
        let cat = match &self.ui.screen {
            Screen::UnifiedList { filter, .. } => filter.category,
            _ => return,
        };
        self.rebuild_unified(Filter {
            category: cat,
            query: query_text,
        });
    }

    fn handle_unified_list(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::MoveUp => {
                self.move_up();
                vec![]
            }
            Action::MoveDown => {
                self.move_down();
                vec![]
            }
            Action::MoveToTop => {
                self.move_to_top();
                vec![]
            }
            Action::MoveToBottom => {
                self.move_to_bottom();
                vec![]
            }
            Action::MovePageUp => {
                self.move_page_up();
                vec![]
            }
            Action::MovePageDown => {
                self.move_page_down();
                vec![]
            }
            Action::Enter => {
                let ea = compute_enter_action(self);
                self.apply_enter_action(ea)
            }
            Action::Investigate => self.handle_investigate(),
            _ => unreachable!(),
        }
    }

    fn handle_detail(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::MoveUp => {
                self.move_up();
                vec![]
            }
            Action::MoveDown => {
                self.move_down();
                vec![]
            }
            Action::MoveToTop => {
                self.move_to_top();
                vec![]
            }
            Action::MoveToBottom => {
                self.move_to_bottom();
                vec![]
            }
            Action::MovePageUp => {
                self.move_page_up();
                vec![]
            }
            Action::MovePageDown => {
                self.move_page_down();
                vec![]
            }
            Action::Enter => {
                let ea = compute_enter_action(self);
                self.apply_enter_action(ea)
            }
            Action::Investigate => self.handle_investigate(),
            _ => unreachable!(),
        }
    }

    fn handle_investigate(&mut self) -> Vec<Effect> {
        match compute_investigate_action(self) {
            InvestigateAction::LaunchCi { repo, run_url } => {
                vec![Effect::LaunchCi { repo, run_url }]
            }
            InvestigateAction::LaunchIssue { repo, number } => {
                vec![Effect::LaunchIssue { repo, number }]
            }
            InvestigateAction::LaunchPr {
                repo,
                number,
                kind,
                author,
                review_decision,
                head_branch,
                base_branch,
            } => vec![Effect::LaunchPr {
                repo,
                number,
                kind,
                author,
                review_decision,
                head_branch,
                base_branch,
            }],
            InvestigateAction::LaunchLoki {
                project,
                env,
                title,
                message,
                line,
            } => vec![Effect::LaunchLoki {
                project,
                env,
                title,
                message,
                line,
            }],
            #[cfg(feature = "private")]
            InvestigateAction::LaunchMediaBlocked { title, error } => {
                vec![Effect::LaunchMediaBlocked { title, error }]
            }
            InvestigateAction::None => {
                self.ui.flash = Some("No investigation mapped".to_string());
                vec![]
            }
        }
    }

    fn apply_enter_action(&mut self, ea: EnterAction) -> Vec<Effect> {
        match ea {
            EnterAction::None => vec![],
            EnterAction::OpenUrl(url) => vec![Effect::OpenUrl(url)],
            EnterAction::OpenDetail {
                group_index,
                item_count,
            } => {
                let Screen::UnifiedList {
                    items,
                    selected,
                    filter,
                } = &self.ui.screen
                else {
                    return vec![];
                };
                let snapshot = ListSnapshot {
                    items: items.clone(),
                    selected: *selected,
                    filter: filter.clone(),
                };
                assert!(
                    matches!(items.get(group_index), Some(DisplayItem::Group { .. })),
                    "group_index must point to a Group variant"
                );
                let mut ds = ListState::default();
                if item_count > 0 {
                    ds.select(Some(0));
                }
                self.ui.screen = Screen::Detail {
                    parent: snapshot,
                    view: DetailView {
                        group_index,
                        list_state: ds,
                    },
                };
                vec![]
            }
        }
    }

    pub(crate) fn selected_url(&self) -> Option<&str> {
        match &self.ui.screen {
            Screen::UnifiedList {
                items, selected, ..
            } => match items.get(*selected)? {
                DisplayItem::Single(item) => item_url(item),
                DisplayItem::Group { .. } => None,
            },
            Screen::Detail { parent, view } => {
                let sel = view.list_state.selected().unwrap_or(0);
                match parent.items.get(view.group_index)? {
                    DisplayItem::Group { items, .. } => items.get(sel).and_then(item_url),
                    _ => None,
                }
            }
        }
    }
}

pub(crate) fn compute_enter_action(app: &App) -> EnterAction {
    match app.current_screen() {
        Screen::UnifiedList {
            items, selected, ..
        } => match items.get(*selected) {
            Some(DisplayItem::Group {
                items: group_items, ..
            }) => EnterAction::OpenDetail {
                group_index: *selected,
                item_count: group_items.len(),
            },
            Some(DisplayItem::Single(_)) => app
                .selected_url()
                .map(|u| EnterAction::OpenUrl(u.to_string()))
                .unwrap_or(EnterAction::None),
            None => EnterAction::None,
        },
        Screen::Detail { .. } => app
            .selected_url()
            .map(|u| EnterAction::OpenUrl(u.to_string()))
            .unwrap_or(EnterAction::None),
    }
}

pub(crate) fn compute_investigate_action(app: &App) -> InvestigateAction {
    let Some(item) = app.ui.screen.selected_status_item() else {
        return InvestigateAction::None;
    };
    match item_investigation(&item) {
        Some(InvestigationKind::Ci { repo, run_url }) => {
            InvestigateAction::LaunchCi { repo, run_url }
        }
        Some(InvestigationKind::Issue { repo, number }) => {
            InvestigateAction::LaunchIssue { repo, number }
        }
        Some(InvestigationKind::Pr {
            repo,
            number,
            kind,
            author,
            review_decision,
            head_branch,
            base_branch,
        }) => InvestigateAction::LaunchPr {
            repo,
            number,
            kind,
            author,
            review_decision,
            head_branch,
            base_branch,
        },
        Some(InvestigationKind::Loki {
            project,
            env,
            title,
            message,
            line,
        }) => InvestigateAction::LaunchLoki {
            project,
            env,
            title,
            message,
            line,
        },
        #[cfg(feature = "private")]
        Some(InvestigationKind::MediaBlocked { title, error }) => {
            InvestigateAction::LaunchMediaBlocked { title, error }
        }
        None => InvestigateAction::None,
    }
}

pub(crate) fn handle_msg(app: &mut App, msg: Msg) -> Result<Vec<Effect>> {
    match msg {
        Msg::Action(action) => Ok(app.update(action)),
        Msg::Tick => {
            if !matches!(app.data.refresh_state, RefreshState::InProgress) {
                app.data.refresh_state = RefreshState::InProgress;
                Ok(vec![Effect::StartRefresh])
            } else {
                Ok(vec![])
            }
        }
        Msg::FetchResult(Ok(report)) => {
            let json =
                serde_json::to_string(&report).context("failed to serialize status report")?;
            let filter = match &app.ui.screen {
                Screen::UnifiedList { filter, .. } => filter.clone(),
                Screen::Detail { parent, .. } => parent.filter.clone(),
            };
            app.data.raw_items = report.items.clone();
            let items = build_unified(report.items, &filter);
            app.ui.screen = Screen::UnifiedList {
                items,
                selected: 0,
                filter,
            };
            app.data.last_updated = Some(Utc::now());
            // If any sources failed, show partial state; otherwise fully idle.
            app.data.refresh_state = if report.errors.is_empty() {
                RefreshState::Idle
            } else {
                RefreshState::Partial(report.errors)
            };
            Ok(vec![Effect::WriteCache(json)])
        }
        Msg::FetchResult(Err(e)) => {
            app.data.refresh_state = RefreshState::Failed(e.to_string());
            Ok(vec![])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compute_investigate_action, handle_msg, Action, App, Effect, InvestigateAction, Msg,
        RefreshState, Screen,
    };
    use crate::display::{DisplayItem, Filter, ListSnapshot};
    use crate::state::{DataState, DetailView, UiState};
    use workflows::status::{StatusItem, StatusReport};

    fn app_with_items(items: Vec<DisplayItem>) -> App {
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items,
                    selected: 0,
                    filter: Filter::default(),
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn app_in_detail(group_items: Vec<StatusItem>) -> App {
        let snapshot = ListSnapshot {
            items: vec![DisplayItem::Group {
                label: "group".to_string(),
                items: group_items,
            }],
            selected: 0,
            filter: Filter::default(),
        };
        App {
            ui: UiState {
                screen: Screen::Detail {
                    parent: snapshot,
                    view: DetailView {
                        group_index: 0,
                        list_state: {
                            let mut ls = ratatui::widgets::ListState::default();
                            ls.select(Some(0));
                            ls
                        },
                    },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn apply(app: &mut App, actions: &[Action]) {
        for action in actions {
            app.update(*action);
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

    #[cfg(feature = "private")]
    fn media_blocked() -> StatusItem {
        StatusItem::MediaBlocked(workflows::private::status::BlockedItem {
            source: "Media".to_string(),
            urgency: domain::Urgency::High,
            age: chrono::Duration::zero(),
            title: "Show — S01E01".to_string(),
            error: "Invalid video file".to_string(),
            url: "http://media-server/queue".to_string(),
        })
    }

    #[test]
    fn investigate_action_launches_ci_from_unified_list_selection() {
        let app = app_with_items(vec![DisplayItem::Single(ci_failure())]);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchCi {
                repo: "ooloth/hub".to_string(),
                run_url: "https://github.com/ooloth/hub/actions/runs/123".to_string(),
            }
        );
    }

    #[test]
    fn investigate_action_launches_ci_from_detail_selection() {
        let app = app_in_detail(vec![ci_failure()]);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchCi {
                repo: "ooloth/hub".to_string(),
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
                age: chrono::Duration::zero(),
                urgency: domain::Urgency::Low,
                labels: vec![],
            },
        ))]);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchIssue {
                repo: "ooloth/hub".to_string(),
                number: 31,
            }
        );
    }

    #[test]
    fn investigate_action_launches_review_code_for_pr_to_review() {
        let app = app_with_items(vec![DisplayItem::Single(StatusItem::Pr(
            domain::PullRequest {
                number: 7,
                title: "add feature".to_string(),
                repo: domain::RepoSlug::new("ooloth", "hub"),
                url: "https://github.com/ooloth/hub/pull/7".to_string(),
                age: chrono::Duration::zero(),
                urgency: domain::Urgency::Low,
                kind: domain::PrKind::ToReview,
                author: "alice".to_string(),
                review_decision: None,
                review_count: 0,
                head_branch: "feat/thing".to_string(),
                base_branch: "main".to_string(),
            },
        ))]);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchPr {
                repo: "ooloth/hub".to_string(),
                number: 7,
                kind: domain::PrKind::ToReview,
                author: "alice".to_string(),
                review_decision: None,
                head_branch: "feat/thing".to_string(),
                base_branch: "main".to_string(),
            }
        );
    }

    #[test]
    fn investigate_action_launches_review_converge_for_own_pr_without_changes_requested() {
        let app = app_with_items(vec![DisplayItem::Single(StatusItem::Pr(
            domain::PullRequest {
                number: 8,
                title: "my feature".to_string(),
                repo: domain::RepoSlug::new("ooloth", "hub"),
                url: "https://github.com/ooloth/hub/pull/8".to_string(),
                age: chrono::Duration::zero(),
                urgency: domain::Urgency::Low,
                kind: domain::PrKind::Mine,
                author: "ooloth".to_string(),
                review_decision: Some(domain::ReviewDecision::Approved),
                review_count: 1,
                head_branch: "feat/mine".to_string(),
                base_branch: "main".to_string(),
            },
        ))]);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchPr {
                repo: "ooloth/hub".to_string(),
                number: 8,
                kind: domain::PrKind::Mine,
                author: "ooloth".to_string(),
                review_decision: Some(domain::ReviewDecision::Approved),
                head_branch: "feat/mine".to_string(),
                base_branch: "main".to_string(),
            }
        );
    }

    #[test]
    fn investigate_action_launches_review_pr_comments_for_own_pr_with_changes_requested() {
        let app = app_with_items(vec![DisplayItem::Single(StatusItem::Pr(
            domain::PullRequest {
                number: 9,
                title: "my draft".to_string(),
                repo: domain::RepoSlug::new("ooloth", "hub"),
                url: "https://github.com/ooloth/hub/pull/9".to_string(),
                age: chrono::Duration::zero(),
                urgency: domain::Urgency::Low,
                kind: domain::PrKind::MyDraft,
                author: "ooloth".to_string(),
                review_decision: Some(domain::ReviewDecision::ChangesRequested),
                review_count: 2,
                head_branch: "feat/draft".to_string(),
                base_branch: "main".to_string(),
            },
        ))]);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchPr {
                repo: "ooloth/hub".to_string(),
                number: 9,
                kind: domain::PrKind::MyDraft,
                author: "ooloth".to_string(),
                review_decision: Some(domain::ReviewDecision::ChangesRequested),
                head_branch: "feat/draft".to_string(),
                base_branch: "main".to_string(),
            }
        );
    }

    #[test]
    fn update_toggle_help_flips_flag() {
        let mut app = App::default();
        app.update(Action::ToggleHelp);
        assert!(app.ui.show_help);
        app.update(Action::ToggleHelp);
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
        app.update(Action::ToggleHelp);
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
    fn enter_on_group_in_unified_list_opens_detail() {
        let mut app = app_with_items(vec![DisplayItem::Group {
            label: "hub".to_string(),
            items: vec![ci_failure()],
        }]);
        app.update(Action::Enter);
        assert!(matches!(app.current_screen(), Screen::Detail { .. }));
    }

    #[test]
    fn back_from_detail_returns_to_unified_list() {
        let mut app = app_with_items(vec![DisplayItem::Group {
            label: "hub".to_string(),
            items: vec![ci_failure()],
        }]);
        apply(&mut app, &[Action::Enter, Action::Back]);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    #[test]
    fn back_from_detail_restores_selection() {
        let mut app = app_with_items(vec![
            DisplayItem::Single(ci_failure()),
            DisplayItem::Group {
                label: "hub".to_string(),
                items: vec![ci_failure()],
            },
        ]);
        // move to second item (the group) then drill in
        apply(&mut app, &[Action::MoveDown, Action::Enter, Action::Back]);
        let Screen::UnifiedList { selected, .. } = app.current_screen() else {
            panic!("expected UnifiedList");
        };
        assert_eq!(*selected, 1);
    }

    #[test]
    fn back_does_nothing_from_unified_list() {
        let mut app = App::default();
        app.update(Action::Back);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    #[test]
    fn full_navigation_round_trip() {
        let mut app = app_with_items(vec![DisplayItem::Group {
            label: "hub".to_string(),
            items: vec![ci_failure()],
        }]);
        apply(&mut app, &[Action::Enter]);
        assert!(matches!(app.current_screen(), Screen::Detail { .. }));
        apply(&mut app, &[Action::Back]);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    #[test]
    fn refresh_action_when_idle_starts_refresh() {
        let mut app = App::default();
        let effects = handle_msg(&mut app, Msg::Action(Action::Refresh)).unwrap();
        assert!(matches!(app.data.refresh_state, RefreshState::InProgress));
        assert!(matches!(effects.as_slice(), [Effect::StartRefresh]));
    }

    #[test]
    fn refresh_action_when_in_progress_does_nothing() {
        let mut app = App {
            data: DataState {
                refresh_state: RefreshState::InProgress,
                ..DataState::default()
            },
            ..App::default()
        };
        let effects = handle_msg(&mut app, Msg::Action(Action::Refresh)).unwrap();
        assert!(matches!(app.data.refresh_state, RefreshState::InProgress));
        assert!(effects.is_empty());
    }

    #[test]
    fn handle_msg_tick_when_idle_starts_refresh() {
        let mut app = App::default();
        let effects = handle_msg(&mut app, Msg::Tick).unwrap();
        assert!(matches!(app.data.refresh_state, RefreshState::InProgress));
        assert!(matches!(effects.as_slice(), [Effect::StartRefresh]));
    }

    #[test]
    fn handle_msg_tick_when_in_progress_does_nothing() {
        let mut app = App {
            data: DataState {
                refresh_state: RefreshState::InProgress,
                ..DataState::default()
            },
            ..App::default()
        };
        let effects = handle_msg(&mut app, Msg::Tick).unwrap();
        assert!(matches!(app.data.refresh_state, RefreshState::InProgress));
        assert!(effects.is_empty());
    }

    #[test]
    fn handle_msg_fetch_ok_sets_idle_and_populates_items() {
        let mut app = App {
            data: DataState {
                refresh_state: RefreshState::InProgress,
                ..DataState::default()
            },
            ..App::default()
        };
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        assert!(matches!(app.data.refresh_state, RefreshState::Idle));
        assert!(matches!(
            app.current_screen(),
            Screen::UnifiedList { items, .. } if !items.is_empty()
        ));
        assert!(app.data.last_updated.is_some());
    }

    #[test]
    fn handle_msg_fetch_ok_resets_to_unified_list() {
        let mut app = app_with_items(vec![DisplayItem::Group {
            label: "hub".to_string(),
            items: vec![ci_failure()],
        }]);
        apply(&mut app, &[Action::Enter]);
        app.data.refresh_state = RefreshState::InProgress;
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    #[test]
    fn handle_msg_fetch_ok_returns_write_cache_effect() {
        let mut app = App::default();
        let effects = handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        assert!(matches!(effects.as_slice(), [Effect::WriteCache(_)]));
    }

    #[test]
    fn handle_msg_fetch_ok_with_errors_sets_partial_state() {
        let mut app = App {
            data: DataState {
                refresh_state: RefreshState::InProgress,
                ..DataState::default()
            },
            ..App::default()
        };
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci_and_errors()))).unwrap();
        assert!(
            matches!(&app.data.refresh_state, RefreshState::Partial(sources) if sources == &["media", "linear issues"])
        );
    }

    #[test]
    fn handle_msg_fetch_ok_with_errors_still_populates_items() {
        let mut app = App::default();
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci_and_errors()))).unwrap();
        assert!(matches!(
            app.current_screen(),
            Screen::UnifiedList { items, .. } if !items.is_empty()
        ));
    }

    #[test]
    fn handle_msg_fetch_ok_with_errors_returns_write_cache_effect() {
        let mut app = App::default();
        let effects =
            handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci_and_errors()))).unwrap();
        assert!(matches!(effects.as_slice(), [Effect::WriteCache(_)]));
    }

    #[test]
    fn handle_msg_fetch_err_sets_failed_state() {
        let mut app = App {
            data: DataState {
                refresh_state: RefreshState::InProgress,
                ..DataState::default()
            },
            ..App::default()
        };
        let effects = handle_msg(
            &mut app,
            Msg::FetchResult(Err(anyhow::anyhow!("network error"))),
        )
        .unwrap();
        assert!(matches!(app.data.refresh_state, RefreshState::Failed(_)));
        assert!(effects.is_empty());
    }

    #[test]
    fn handle_msg_action_delegates_to_update() {
        let mut app = App::default();
        let effects = handle_msg(&mut app, Msg::Action(Action::Quit)).unwrap();
        assert!(matches!(effects.as_slice(), [Effect::Quit]));
    }

    #[cfg(feature = "private")]
    #[test]
    fn investigate_action_launches_media_from_unified_list_selection() {
        let app = app_with_items(vec![DisplayItem::Single(media_blocked())]);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchMediaBlocked {
                title: "Show — S01E01".to_string(),
                error: "Invalid video file".to_string(),
            }
        );
    }

    #[cfg(feature = "private")]
    #[test]
    fn investigate_action_launches_media_from_detail_selection() {
        let app = app_in_detail(vec![media_blocked()]);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchMediaBlocked {
                title: "Show — S01E01".to_string(),
                error: "Invalid video file".to_string(),
            }
        );
    }

    // --- Query mode re-entry ---

    #[test]
    fn start_query_with_no_prior_filter_begins_empty() {
        let mut app = App::default();
        app.update(Action::StartQuery);
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
        app.update(Action::StartQuery);
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

    fn selected(app: &App) -> usize {
        match app.current_screen() {
            Screen::UnifiedList { selected, .. } => *selected,
            _ => panic!("expected UnifiedList"),
        }
    }

    fn items_n(n: usize) -> Vec<DisplayItem> {
        (0..n).map(|_| DisplayItem::Single(ci_failure())).collect()
    }

    #[test]
    fn pending_g_sets_pending_flag() {
        let mut app = App::default();
        app.update(Action::PendingG);
        assert!(app.ui.pending_g);
    }

    #[test]
    fn any_subsequent_action_clears_pending_g() {
        let mut app = app_with_items(items_n(5));
        app.update(Action::PendingG);
        app.update(Action::MoveDown);
        assert!(!app.ui.pending_g);
    }

    #[test]
    fn move_to_top_goes_to_first_item() {
        let mut app = app_with_items(items_n(5));
        apply(&mut app, &[Action::MoveDown, Action::MoveDown]);
        assert_eq!(selected(&app), 2);
        app.update(Action::MoveToTop);
        assert_eq!(selected(&app), 0);
    }

    #[test]
    fn move_to_bottom_goes_to_last_item() {
        let mut app = app_with_items(items_n(5));
        app.update(Action::MoveToBottom);
        assert_eq!(selected(&app), 4);
    }

    #[test]
    fn move_to_bottom_on_empty_list_is_noop() {
        let mut app = app_with_items(vec![]);
        app.update(Action::MoveToBottom);
        assert_eq!(selected(&app), 0);
    }

    #[test]
    fn move_page_down_advances_by_10() {
        let mut app = app_with_items(items_n(25));
        app.update(Action::MovePageDown);
        assert_eq!(selected(&app), 10);
    }

    #[test]
    fn move_page_down_clamps_at_last_item() {
        let mut app = app_with_items(items_n(5));
        app.update(Action::MovePageDown);
        assert_eq!(selected(&app), 4);
    }

    #[test]
    fn move_page_up_retreats_by_10() {
        let mut app = app_with_items(items_n(25));
        apply(&mut app, &[Action::MovePageDown, Action::MovePageDown]);
        assert_eq!(selected(&app), 20);
        app.update(Action::MovePageUp);
        assert_eq!(selected(&app), 10);
    }

    #[test]
    fn move_page_up_clamps_at_first_item() {
        let mut app = app_with_items(items_n(5));
        apply(&mut app, &[Action::MoveDown, Action::MoveDown]);
        app.update(Action::MovePageUp);
        assert_eq!(selected(&app), 0);
    }
}
