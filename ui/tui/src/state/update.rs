use anyhow::{Context, Result};
use chrono::Utc;

use domain::agent_ready_labels;

use super::{
    modal::{cycle_task_kind, seed_from_item, TaskFormField},
    Action, App, DetailMode, Effect, InvestigateAction, Msg, PrOwnership, PrPrevScreen,
    RefreshState, Screen, TaskCreationModal,
};
use crate::display::{
    build_unified, flatten, item_investigation, item_url, Filter, FlatRow, InvestigationKind,
    ListSnapshot,
};
use workflows::status::StatusItem;

impl App {
    pub(crate) fn update(&mut self, action: Action) -> Vec<Effect> {
        self.ui.flash = None;
        self.ui.pending_g = false;
        // Clear the PR-actions submenu on any action that isn't part of the submenu itself.
        if !matches!(
            action,
            Action::PrActionSubmenu
                | Action::OpenPrDiffInDelta
                | Action::OpenInLazygit
                | Action::OpenInOcto
        ) {
            self.ui.pending_pr_action = false;
        }
        // Clear the review picker submenu on any action that isn't part of it.
        if !matches!(action, Action::OpenReviewPicker) {
            self.ui.pending_review_action = false;
        }
        // Clear the task status submenu on any action that isn't part of it.
        if !matches!(
            action,
            Action::TaskStatusSubmenu | Action::TransitionTaskStatus(_)
        ) {
            self.ui.pending_task_status = false;
        }
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
                    vec![Effect::StartRefresh]
                } else {
                    vec![]
                }
            }
            Action::Back => {
                // Collapse the detail pane before any deeper back navigation.
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
                if let Screen::UnifiedList { filter, .. } = &mut self.ui.screen {
                    let cat = filter.category;
                    self.rebuild_unified(Filter {
                        category: cat,
                        query: None,
                    });
                }
                vec![]
            }
            Action::PendingG => {
                self.ui.pending_g = true;
                vec![]
            }
            Action::ScrollDetailDown | Action::ScrollDetailUp => {
                // Handled in handle_unified_list when detail_mode is Visible.
                match &self.ui.screen {
                    Screen::UnifiedList { .. } => self.handle_unified_list(action),
                    _ => vec![],
                }
            }
            Action::PrActionSubmenu => {
                self.ui.pending_pr_action = true;
                vec![]
            }
            // CancelPrSubmenu dismisses the d-submenu without collapsing the split view.
            // pending_pr_action is already cleared by the prologue for any non-submenu action.
            Action::CancelPrSubmenu => vec![],
            Action::TaskStatusSubmenu => {
                if let Some(StatusItem::AgentSession(_)) = self.ui.screen.selected_status_item() {
                    self.ui.pending_task_status = true;
                }
                vec![]
            }
            // CancelTaskStatusSubmenu dismisses the submenu; flag already cleared by prologue.
            Action::CancelTaskStatusSubmenu => vec![],
            Action::TransitionTaskStatus(status) => {
                if let Some(StatusItem::AgentSession(task)) = self.ui.screen.selected_status_item()
                {
                    vec![Effect::UpdateTaskStatus {
                        id: task.id,
                        status,
                    }]
                } else {
                    vec![]
                }
            }
            Action::OpenTaskCreationForm => {
                let seed = self
                    .ui
                    .screen
                    .selected_item_for_seeding()
                    .and_then(|item| seed_from_item(&item));
                self.ui.modal = Some(TaskCreationModal::with_seed(seed));
                vec![]
            }
            Action::OpenBlankTaskCreationForm => {
                self.ui.modal = Some(TaskCreationModal::with_seed(None));
                vec![]
            }
            Action::CancelTaskCreation => {
                self.ui.modal = None;
                vec![]
            }
            Action::FocusNextField => {
                if let Some(modal) = &mut self.ui.modal {
                    modal.focused_field = modal.focused_field.next();
                }
                vec![]
            }
            Action::FocusPrevField => {
                if let Some(modal) = &mut self.ui.modal {
                    modal.focused_field = modal.focused_field.prev();
                }
                vec![]
            }
            Action::CycleTaskKind => {
                if let Some(modal) = &mut self.ui.modal {
                    modal.kind = cycle_task_kind(modal.kind);
                }
                vec![]
            }
            Action::ModalTextInput(key) => {
                if let Some(modal) = &mut self.ui.modal {
                    match modal.focused_field {
                        TaskFormField::Title => {
                            modal.title.input(key);
                        }
                        TaskFormField::Description => {
                            modal.description.input(key);
                        }
                        TaskFormField::Link => {
                            modal.link.input(key);
                        }
                        TaskFormField::Kind | TaskFormField::Submit => {}
                    }
                }
                vec![]
            }
            Action::CommitTaskCreation => {
                let result = self.ui.modal.as_ref().and_then(|m| m.try_into_request());
                match result {
                    None => {
                        if self.ui.modal.is_some() {
                            self.ui.flash = Some("Title required".to_string());
                        }
                        vec![]
                    }
                    Some(req) => {
                        self.ui.modal = None;
                        vec![Effect::CreateTask {
                            title: req.title.as_str().to_owned(),
                            description: req.description,
                            kind: req.kind,
                            links: req.links,
                        }]
                    }
                }
            }
            Action::OpenPrDiffInDelta => {
                self.ui.pending_pr_action = false;
                if let Some(StatusItem::Pr(pr)) = self.ui.screen.selected_status_item() {
                    vec![Effect::OpenPrDiffInDelta {
                        repo: pr.repo.to_string(),
                        number: pr.number,
                    }]
                } else {
                    vec![]
                }
            }
            Action::OpenInOcto => {
                self.ui.pending_pr_action = false;
                if let Some(StatusItem::Pr(pr)) = self.ui.screen.selected_status_item() {
                    vec![Effect::OpenInOcto {
                        repo: pr.repo.to_string(),
                        number: pr.number,
                        head_branch: pr.head_branch.clone(),
                    }]
                } else {
                    vec![]
                }
            }
            Action::OpenInLazygit => {
                self.ui.pending_pr_action = false;
                if let Some(StatusItem::Pr(pr)) = self.ui.screen.selected_status_item() {
                    vec![Effect::OpenInLazygit {
                        repo: pr.repo.to_string(),
                        number: pr.number,
                        head_branch: pr.head_branch.clone(),
                    }]
                } else {
                    vec![]
                }
            }
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

    fn rebuild_unified(&mut self, new_filter: Filter) {
        let items = build_unified(self.data.raw_items.clone(), &new_filter);
        if let Screen::UnifiedList {
            items: ref mut i,
            flat_rows: ref mut fr,
            selected: ref mut s,
            filter: ref mut f,
            expanded_groups: ref eg,
            ..
        } = self.ui.screen
        {
            *fr = flatten(&items, eg);
            *i = items;
            *s = 0;
            *f = new_filter;
        }
    }

    fn sync_query_to_filter(&mut self) {
        let query_text = self.ui.query_input.clone().filter(|q| !q.is_empty());
        if let Screen::UnifiedList { filter, .. } = &mut self.ui.screen {
            let cat = filter.category;
            self.rebuild_unified(Filter {
                category: cat,
                query: query_text,
            });
        }
    }

    fn reset_detail_scroll(&mut self) {
        if let Screen::UnifiedList {
            detail_mode: DetailMode::Visible { detail_scroll },
            ..
        } = &mut self.ui.screen
        {
            *detail_scroll = 0;
        }
    }

    fn handle_unified_list(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::MoveUp => {
                self.move_up();
                self.reset_detail_scroll();
                vec![]
            }
            Action::MoveDown => {
                self.move_down();
                self.reset_detail_scroll();
                vec![]
            }
            Action::MoveToTop => {
                self.move_to_top();
                self.reset_detail_scroll();
                vec![]
            }
            Action::MoveToBottom => {
                self.move_to_bottom();
                self.reset_detail_scroll();
                vec![]
            }
            Action::MovePageUp => {
                self.move_page_up();
                self.reset_detail_scroll();
                vec![]
            }
            Action::MovePageDown => {
                self.move_page_down();
                self.reset_detail_scroll();
                vec![]
            }
            Action::Enter => {
                let is_hidden = matches!(
                    &self.ui.screen,
                    Screen::UnifiedList {
                        detail_mode: DetailMode::Hidden,
                        ..
                    }
                );
                if is_hidden {
                    if let Screen::UnifiedList {
                        detail_mode,
                        flat_rows,
                        selected,
                        ..
                    } = &mut self.ui.screen
                    {
                        let is_agent = matches!(
                            flat_rows.get(*selected),
                            Some(FlatRow::Single(StatusItem::AgentSession(_)))
                        );
                        let initial_scroll = if is_agent { u16::MAX } else { 0 };
                        *detail_mode = DetailMode::Visible {
                            detail_scroll: initial_scroll,
                        };
                    }
                }
                vec![]
            }
            Action::ScrollDetailDown => {
                if let Screen::UnifiedList {
                    detail_mode: DetailMode::Visible { detail_scroll },
                    ..
                } = &mut self.ui.screen
                {
                    *detail_scroll = detail_scroll.saturating_add(1);
                }
                vec![]
            }
            Action::ScrollDetailUp => {
                if let Screen::UnifiedList {
                    detail_mode: DetailMode::Visible { detail_scroll },
                    ..
                } = &mut self.ui.screen
                {
                    *detail_scroll = detail_scroll.saturating_sub(1);
                }
                vec![]
            }
            Action::ExpandGroup => {
                let to_expand = {
                    let Screen::UnifiedList {
                        flat_rows,
                        selected,
                        ..
                    } = &self.ui.screen
                    else {
                        return vec![];
                    };
                    match flat_rows.get(*selected) {
                        Some(FlatRow::GroupHeader {
                            key,
                            expanded: false,
                            ..
                        }) => Some(key.clone()),
                        _ => None,
                    }
                };
                if let Some(key) = to_expand {
                    if let Screen::UnifiedList {
                        items,
                        flat_rows,
                        expanded_groups,
                        ..
                    } = &mut self.ui.screen
                    {
                        expanded_groups.insert(key);
                        *flat_rows = flatten(items, expanded_groups);
                    }
                }
                vec![]
            }
            Action::CollapseGroup => {
                let to_collapse = {
                    let Screen::UnifiedList {
                        flat_rows,
                        selected,
                        expanded_groups,
                        ..
                    } = &self.ui.screen
                    else {
                        return vec![];
                    };
                    match flat_rows.get(*selected) {
                        Some(FlatRow::GroupHeader {
                            key,
                            expanded: true,
                            ..
                        }) => Some((key.clone(), false)),
                        Some(FlatRow::GroupChild { parent_key, .. })
                            if expanded_groups.contains(parent_key) =>
                        {
                            Some((parent_key.clone(), true))
                        }
                        _ => None,
                    }
                };
                if let Some((key, jump_to_parent)) = to_collapse {
                    if let Screen::UnifiedList {
                        items,
                        flat_rows,
                        selected,
                        expanded_groups,
                        ..
                    } = &mut self.ui.screen
                    {
                        expanded_groups.remove(&key);
                        *flat_rows = flatten(items, expanded_groups);
                        if jump_to_parent {
                            if let Some(idx) = flat_rows.iter().position(
                                |r| matches!(r, FlatRow::GroupHeader { key: k, .. } if k == &key),
                            ) {
                                *selected = idx;
                            }
                        }
                    }
                }
                vec![]
            }
            Action::OpenUrl => {
                if let Some(url) = self.selected_url() {
                    vec![Effect::OpenUrl(url.to_string())]
                } else {
                    vec![]
                }
            }
            Action::OpenReviewPicker => {
                self.ui.pending_review_action = true;
                vec![]
            }
            Action::CommitReview(skill) => {
                let Some(StatusItem::Pr(pr)) = self.ui.screen.selected_status_item() else {
                    return vec![];
                };
                let ownership = PrOwnership::from_kind(pr.kind);
                vec![Effect::ReviewPr {
                    repo: pr.repo.to_string(),
                    number: pr.number,
                    ownership,
                    skill,
                    head_branch: pr.head_branch.clone(),
                }]
            }
            Action::CancelReview => vec![],
            Action::MergePr => {
                let Some(StatusItem::Pr(pr)) = self.ui.screen.selected_status_item() else {
                    return vec![];
                };
                let Screen::UnifiedList {
                    items,
                    selected,
                    filter,
                    expanded_groups,
                    detail_mode,
                    ..
                } = &self.ui.screen
                else {
                    return vec![];
                };
                let snapshot = ListSnapshot {
                    items: items.clone(),
                    selected: *selected,
                    filter: filter.clone(),
                    expanded_groups: expanded_groups.clone(),
                    detail_mode: detail_mode.clone(),
                };
                self.ui.screen = Screen::MergingPr {
                    parent: snapshot.clone(),
                    pr,
                    prev: PrPrevScreen::UnifiedList { snapshot },
                };
                vec![]
            }
            Action::ApproveForAgent => {
                let Some(StatusItem::Issue(issue)) = self.ui.screen.selected_status_item() else {
                    return vec![];
                };
                let labels = agent_ready_labels(&issue.labels);
                vec![Effect::SetIssueLabels {
                    repo: issue.repo.to_string(),
                    number: issue.number,
                    labels,
                }]
            }
            Action::Investigate => self.handle_investigate(),
            _ => unreachable!(),
        }
    }

    fn handle_merging_pr(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::CancelMerge => {
                let Screen::MergingPr { prev, .. } = std::mem::take(&mut self.ui.screen) else {
                    return vec![];
                };
                let PrPrevScreen::UnifiedList { snapshot } = prev;
                self.ui.screen = Screen::UnifiedList {
                    flat_rows: flatten(&snapshot.items, &snapshot.expanded_groups),
                    items: snapshot.items,
                    selected: snapshot.selected,
                    filter: snapshot.filter,
                    expanded_groups: snapshot.expanded_groups,
                    detail_mode: snapshot.detail_mode,
                };
                vec![]
            }
            Action::CommitMerge => {
                let Screen::MergingPr { pr, prev, .. } = std::mem::take(&mut self.ui.screen) else {
                    return vec![];
                };
                let repo = pr.repo.to_string();
                let number = pr.number;
                let PrPrevScreen::UnifiedList { snapshot } = prev;
                self.ui.screen = Screen::UnifiedList {
                    flat_rows: flatten(&snapshot.items, &snapshot.expanded_groups),
                    items: snapshot.items,
                    selected: snapshot.selected,
                    filter: snapshot.filter,
                    expanded_groups: snapshot.expanded_groups,
                    detail_mode: snapshot.detail_mode,
                };
                vec![Effect::MergePullRequest { repo, number }]
            }
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
                review_decision,
                head_branch,
                ..
            } => vec![Effect::LaunchPr {
                repo,
                number,
                kind,
                review_decision,
                head_branch,
            }],
            InvestigateAction::LaunchGcp {
                project,
                env,
                title,
                message,
                line,
                url,
                lookback,
                gcp_project,
            } => vec![Effect::LaunchGcp {
                project,
                env,
                title,
                message,
                line,
                url,
                lookback,
                gcp_project,
            }],
            InvestigateAction::LaunchLoki {
                project,
                env,
                title,
                message,
                line,
                url,
                lookback,
            } => vec![Effect::LaunchLoki {
                project,
                env,
                title,
                message,
                line,
                url,
                lookback,
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

    pub(crate) fn selected_url(&self) -> Option<&str> {
        match &self.ui.screen {
            Screen::UnifiedList {
                flat_rows,
                selected,
                ..
            } => match flat_rows.get(*selected)? {
                FlatRow::Single(item) => item_url(item),
                FlatRow::GroupChild { item, .. } => item_url(item),
                FlatRow::GroupHeader { .. } => None,
            },
            Screen::MergingPr { pr, .. } => Some(&pr.url),
        }
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
        Some(InvestigationKind::Gcp {
            project,
            env,
            title,
            message,
            line,
            url,
            lookback,
            gcp_project,
        }) => InvestigateAction::LaunchGcp {
            project,
            env,
            title,
            message,
            line,
            url,
            lookback,
            gcp_project,
        },
        Some(InvestigationKind::Loki {
            project,
            env,
            title,
            message,
            line,
            url,
            lookback,
        }) => InvestigateAction::LaunchLoki {
            project,
            env,
            title,
            message,
            line,
            url,
            lookback,
        },
        #[cfg(feature = "private")]
        Some(InvestigationKind::MediaBlocked { title, error }) => {
            InvestigateAction::LaunchMediaBlocked { title, error }
        }
        None => InvestigateAction::None,
    }
}

fn refresh_screen_in_place(screen: &mut Screen, raw: &[workflows::status::StatusItem]) {
    match screen {
        Screen::UnifiedList {
            items,
            flat_rows,
            selected,
            filter,
            expanded_groups,
            ..
        } => {
            let new_items = build_unified(raw.to_vec(), filter);
            *selected = (*selected).min(new_items.len().saturating_sub(1));
            *flat_rows = flatten(&new_items, expanded_groups);
            *items = new_items;
        }
        Screen::MergingPr { parent, .. } => {
            let new_items = build_unified(raw.to_vec(), &parent.filter);
            parent.selected = parent.selected.min(new_items.len().saturating_sub(1));
            parent.items = new_items;
        }
    }
}

fn apply_report(
    app: &mut App,
    report: workflows::status::StatusReport,
    refreshed_at: chrono::DateTime<Utc>,
) {
    app.data.last_updated = Some(refreshed_at);
    app.data.refresh_state = if report.errors.is_empty() {
        RefreshState::Idle
    } else {
        RefreshState::Partial(report.errors)
    };
    app.data.raw_items = report.items;
    refresh_screen_in_place(&mut app.ui.screen, &app.data.raw_items);
}

fn apply_refresh(app: &mut App, report: workflows::status::StatusReport) -> Result<Vec<Effect>> {
    let json = serde_json::to_string(&report).context("failed to serialize status report")?;
    apply_report(app, report, Utc::now());
    Ok(vec![Effect::WriteCache(json)])
}

pub(crate) fn handle_msg(app: &mut App, msg: Msg) -> Result<Vec<Effect>> {
    match msg {
        Msg::Action(action) => Ok(app.update(action)),
        Msg::Tick => {
            if !matches!(app.data.refresh_state, RefreshState::InProgress) {
                Ok(vec![Effect::StartRefresh])
            } else {
                Ok(vec![])
            }
        }
        Msg::FetchResult(Ok(report)) => apply_refresh(app, report),
        Msg::FetchResult(Err(e)) => {
            app.data.refresh_state = RefreshState::Failed(e.to_string());
            Ok(vec![])
        }
        Msg::AppliedFromCache {
            report,
            refreshed_at,
        } => {
            apply_report(app, report, refreshed_at);
            Ok(vec![])
        }
        Msg::StreamUpdate(blocks) => {
            app.data.stream_blocks = blocks;
            Ok(vec![])
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        compute_investigate_action, handle_msg, Action, App, DetailMode, Effect, InvestigateAction,
        Msg, RefreshState, Screen,
    };
    use crate::display::{flatten, Category, DisplayItem, Filter, GroupKey};
    use crate::state::{DataState, UiState};
    use workflows::status::{StatusItem, StatusReport};

    fn app_with_items(items: Vec<DisplayItem>) -> App {
        let expanded = HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    flat_rows,
                    items,
                    selected: 0,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                    detail_mode: DetailMode::Hidden,
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
                approval_count: 0,
                comment_count: 0,
                head_branch: "feat/thing".to_string(),
                base_branch: "main".to_string(),
                body: None,
                ci_status: None,
                changed_files: vec![],
                total_changed_files: 0,
                review_threads: vec![],
                pr_comments: vec![],
                merge_blocker: None,
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
                approval_count: 1,
                comment_count: 0,
                head_branch: "feat/mine".to_string(),
                base_branch: "main".to_string(),
                body: None,
                ci_status: None,
                changed_files: vec![],
                total_changed_files: 0,
                review_threads: vec![],
                pr_comments: vec![],
                merge_blocker: None,
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
                approval_count: 0,
                comment_count: 0,
                head_branch: "feat/draft".to_string(),
                base_branch: "main".to_string(),
                body: None,
                ci_status: None,
                changed_files: vec![],
                total_changed_files: 0,
                review_threads: vec![],
                pr_comments: vec![],
                merge_blocker: None,
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

    fn gcp_item() -> StatusItem {
        StatusItem::Gcp(domain::GcpEntry {
            title: "errors".to_string(),
            project: "mapapp".to_string(),
            env: "neuro".to_string(),
            message: "something broke".to_string(),
            line: r#"{"message":"something broke"}"#.to_string(),
            lookback: "7d".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://console.cloud.google.com/logs/query".to_string(),
            gcp_project: "mapapp-prod-abc123".to_string(),
        })
    }

    #[test]
    fn enter_on_ci_group_header_is_noop() {
        let mut app = app_with_items(vec![DisplayItem::Group {
            label: GroupKey::new("hub".to_string()),
            items: vec![ci_failure(), ci_failure()],
        }]);
        app.update(Action::Enter);
        let len = match app.current_screen() {
            Screen::UnifiedList { flat_rows, .. } => flat_rows.len(),
            _ => panic!(),
        };
        assert_eq!(len, 1);
    }

    #[test]
    fn enter_on_gcp_group_header_activates_split_view() {
        let mut app = app_with_items(vec![DisplayItem::Group {
            label: GroupKey::new("hub".to_string()),
            items: vec![gcp_item(), gcp_item()],
        }]);
        app.update(Action::Enter);
        match app.current_screen() {
            Screen::UnifiedList { detail_mode, .. } => {
                assert_eq!(detail_mode, &DetailMode::Visible { detail_scroll: 0 });
            }
            _ => panic!("expected UnifiedList"),
        }
    }

    #[test]
    fn expand_group_expands_collapsed_header() {
        let mut app = app_with_items(vec![DisplayItem::Group {
            label: GroupKey::new("hub".to_string()),
            items: vec![ci_failure(), ci_failure()],
        }]);
        assert_eq!(app.active_list_len(), 1);
        app.update(Action::ExpandGroup);
        assert_eq!(app.active_list_len(), 3);
    }

    #[test]
    fn expand_group_on_expanded_header_is_noop() {
        let mut app = app_with_items(vec![DisplayItem::Group {
            label: GroupKey::new("hub".to_string()),
            items: vec![ci_failure(), ci_failure()],
        }]);
        app.update(Action::ExpandGroup);
        assert_eq!(app.active_list_len(), 3);
        app.update(Action::ExpandGroup);
        assert_eq!(app.active_list_len(), 3);
    }

    #[test]
    fn collapse_group_collapses_expanded_header() {
        let mut app = app_with_items(vec![DisplayItem::Group {
            label: GroupKey::new("hub".to_string()),
            items: vec![ci_failure(), ci_failure()],
        }]);
        app.update(Action::ExpandGroup);
        assert_eq!(app.active_list_len(), 3);
        app.update(Action::CollapseGroup);
        assert_eq!(app.active_list_len(), 1);
    }

    #[test]
    fn collapse_group_on_child_jumps_to_header() {
        let mut app = app_with_items(vec![DisplayItem::Group {
            label: GroupKey::new("hub".to_string()),
            items: vec![ci_failure(), ci_failure()],
        }]);
        app.update(Action::ExpandGroup);
        // Move to first child (index 1)
        app.update(Action::MoveDown);
        let before_selected = match app.current_screen() {
            Screen::UnifiedList { selected, .. } => *selected,
            _ => panic!(),
        };
        assert_eq!(before_selected, 1);
        app.update(Action::CollapseGroup);
        // Should collapse and jump back to header at index 0
        let after_selected = match app.current_screen() {
            Screen::UnifiedList { selected, .. } => *selected,
            _ => panic!(),
        };
        assert_eq!(after_selected, 0);
        assert_eq!(app.active_list_len(), 1);
    }

    #[test]
    fn back_from_log_detail_returns_to_unified_list() {
        let loki = StatusItem::Loki(domain::LokiEntry {
            title: "Error spike".to_string(),
            project: "hub".to_string(),
            env: "prod".to_string(),
            message: "timeout".to_string(),
            line: "{}".to_string(),
            lookback: "1h".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://grafana.example.com".to_string(),
        });
        let mut app = app_with_items(vec![DisplayItem::Single(loki)]);
        apply(&mut app, &[Action::Enter, Action::Back]);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    #[test]
    fn back_does_nothing_from_unified_list() {
        let mut app = App::default();
        app.update(Action::Back);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    #[test]
    fn refresh_action_when_idle_starts_refresh() {
        let mut app = App::default();
        let effects = handle_msg(&mut app, Msg::Action(Action::Refresh)).unwrap();
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
    fn handle_msg_applied_from_cache_sets_idle_and_uses_cache_timestamp() {
        use chrono::{Duration, Utc};
        let mut app = App {
            data: DataState {
                refresh_state: RefreshState::InProgress,
                ..DataState::default()
            },
            ..App::default()
        };
        let cache_ts = Utc::now() - Duration::minutes(5);
        let effects = handle_msg(
            &mut app,
            Msg::AppliedFromCache {
                report: report_with_ci(),
                refreshed_at: cache_ts,
            },
        )
        .unwrap();
        assert!(matches!(app.data.refresh_state, RefreshState::Idle));
        assert_eq!(app.data.last_updated, Some(cache_ts));
        assert!(matches!(
            app.current_screen(),
            Screen::UnifiedList { items, .. } if !items.is_empty()
        ));
        assert!(
            effects.is_empty(),
            "AppliedFromCache must not emit effects (e.g. no WriteCache)"
        );
    }

    #[test]
    fn handle_msg_applied_from_cache_with_errors_sets_partial_state() {
        use chrono::Utc;
        let mut app = App::default();
        handle_msg(
            &mut app,
            Msg::AppliedFromCache {
                report: report_with_ci_and_errors(),
                refreshed_at: Utc::now(),
            },
        )
        .unwrap();
        assert!(
            matches!(&app.data.refresh_state, RefreshState::Partial(sources) if sources == &["media", "linear issues"])
        );
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

    // --- Enter / Back on UnifiedList items ---

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

    #[test]
    fn enter_on_issue_in_unified_list_activates_split_view() {
        let mut app = app_with_items(vec![DisplayItem::Single(stub_issue())]);
        app.update(Action::Enter);
        match app.current_screen() {
            Screen::UnifiedList { detail_mode, .. } => {
                assert_eq!(detail_mode, &DetailMode::Visible { detail_scroll: 0 });
            }
            _ => panic!("expected UnifiedList"),
        }
    }

    #[test]
    fn enter_on_ci_in_unified_list_activates_split_view() {
        let mut app = app_with_items(vec![DisplayItem::Single(ci_failure())]);
        app.update(Action::Enter);
        match app.current_screen() {
            Screen::UnifiedList { detail_mode, .. } => {
                assert_eq!(detail_mode, &DetailMode::Visible { detail_scroll: 0 });
            }
            _ => panic!("expected UnifiedList"),
        }
    }

    #[test]
    fn back_from_issue_detail_returns_to_unified_list() {
        let mut app = app_with_items(vec![DisplayItem::Single(stub_issue())]);
        apply(&mut app, &[Action::Enter, Action::Back]);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    #[test]
    fn back_from_issue_detail_restores_selection() {
        let mut app = app_with_items(vec![
            DisplayItem::Single(ci_failure()),
            DisplayItem::Single(stub_issue()),
        ]);
        apply(&mut app, &[Action::MoveDown, Action::Enter, Action::Back]);
        let Screen::UnifiedList { selected, .. } = app.current_screen() else {
            panic!("expected UnifiedList");
        };
        assert_eq!(*selected, 1);
    }

    // --- ApproveForAgent / DismissIssue from split view ---

    fn split_view_app_with_issue() -> App {
        let item = stub_issue();
        let items = vec![DisplayItem::Single(item)];
        let expanded = HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    flat_rows,
                    items,
                    selected: 0,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                    detail_mode: DetailMode::Visible { detail_scroll: 0 },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    #[test]
    fn approve_for_agent_from_split_view_emits_set_issue_labels() {
        let mut app = split_view_app_with_issue();
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

    // --- UnifiedList refresh ---

    fn report_with_two_items() -> StatusReport {
        StatusReport {
            items: vec![ci_failure(), stub_issue()],
            errors: vec![],
        }
    }

    #[test]
    fn refresh_in_unified_list_preserves_selection_when_list_unchanged() {
        let mut app = app_with_items(vec![
            DisplayItem::Single(ci_failure()),
            DisplayItem::Single(stub_issue()),
        ]);
        apply(&mut app, &[Action::MoveDown]); // cursor → 1
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_two_items()))).unwrap();
        let Screen::UnifiedList { selected, .. } = app.current_screen() else {
            panic!("expected UnifiedList");
        };
        assert_eq!(*selected, 1);
    }

    #[test]
    fn refresh_in_unified_list_clamps_selection_when_list_shrinks() {
        let mut app = app_with_items(vec![
            DisplayItem::Single(ci_failure()),
            DisplayItem::Single(ci_failure()),
            DisplayItem::Single(ci_failure()),
            DisplayItem::Single(stub_issue()),
        ]);
        apply(&mut app, &[Action::MoveToBottom]); // cursor → 3
                                                  // report has 2 items → new len 2, cursor clamps to 1
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_two_items()))).unwrap();
        let Screen::UnifiedList { selected, .. } = app.current_screen() else {
            panic!("expected UnifiedList");
        };
        assert_eq!(*selected, 1);
    }

    #[test]
    fn refresh_in_unified_list_applies_current_filter() {
        let mut app = App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items: vec![],
                    flat_rows: vec![],
                    selected: 0,
                    filter: Filter {
                        category: Some(Category::Issues),
                        query: None,
                    },
                    expanded_groups: HashSet::new(),
                    detail_mode: DetailMode::Hidden,
                },
                ..UiState::default()
            },
            ..App::default()
        };
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_two_items()))).unwrap();
        let Screen::UnifiedList { items, .. } = app.current_screen() else {
            panic!("expected UnifiedList");
        };
        assert_eq!(items.len(), 1);
        assert!(matches!(
            &items[0],
            DisplayItem::Single(workflows::status::StatusItem::Issue(_))
        ));
    }

    fn stub_pr() -> StatusItem {
        StatusItem::Pr(domain::PullRequest {
            number: 7,
            title: "Test PR".to_string(),
            repo: domain::RepoSlug::new("ooloth", "hub"),
            url: "https://github.com/ooloth/hub/pull/7".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            kind: domain::PrKind::Mine,
            author: "ooloth".to_string(),
            review_decision: None,
            approval_count: 0,
            comment_count: 0,
            head_branch: "feat/thing".to_string(),
            base_branch: "main".to_string(),
            body: Some("PR body".to_string()),
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: None,
        })
    }

    fn split_view_app_with_pr_for_merge() -> App {
        let item = stub_pr();
        let items = vec![DisplayItem::Single(item)];
        let expanded = HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    flat_rows,
                    items,
                    selected: 0,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                    detail_mode: DetailMode::Visible { detail_scroll: 0 },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    // --- MergePr flow ---

    fn app_in_merging() -> App {
        let mut app = split_view_app_with_pr_for_merge();
        app.update(Action::MergePr);
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
        app.update(Action::CancelMerge);
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
        app.update(Action::CommitMerge);
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

    // --- MergingPr refresh ---

    #[test]
    fn refresh_in_merging_preserves_screen_variant() {
        let mut app = app_in_merging();
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        assert!(matches!(app.current_screen(), Screen::MergingPr { .. }));
    }

    #[test]
    fn refresh_in_merging_updates_parent_items() {
        let mut app = app_in_merging();
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        let Screen::MergingPr { parent, .. } = app.current_screen() else {
            panic!("expected MergingPr");
        };
        assert!(matches!(
            parent.items[0],
            DisplayItem::Single(workflows::status::StatusItem::Ci(_))
        ));
    }

    // --- Task creation modal ---

    fn modal_app() -> App {
        use crate::state::TaskCreationModal;
        App {
            ui: UiState {
                modal: Some(TaskCreationModal::blank()),
                ..UiState::default()
            },
            ..App::default()
        }
    }

    #[test]
    fn open_task_creation_form_sets_modal() {
        let mut app = App::default();
        app.update(Action::OpenTaskCreationForm);
        assert!(app.ui.modal.is_some());
    }

    #[test]
    fn open_blank_task_creation_form_sets_modal_with_no_seed() {
        let mut app = App::default();
        app.update(Action::OpenBlankTaskCreationForm);
        let modal = app.ui.modal.as_ref().unwrap();
        assert!(modal.title.lines().join("").is_empty());
        assert!(modal.description.lines().join("").is_empty());
        assert_eq!(modal.kind, domain::TaskKind::Implement);
    }

    #[test]
    fn cancel_task_creation_clears_modal() {
        let mut app = modal_app();
        app.update(Action::CancelTaskCreation);
        assert!(app.ui.modal.is_none());
    }

    #[test]
    fn focus_next_field_advances_from_title_to_description() {
        use crate::state::TaskFormField;
        let mut app = modal_app();
        app.update(Action::FocusNextField);
        assert_eq!(
            app.ui.modal.as_ref().unwrap().focused_field,
            TaskFormField::Description
        );
    }

    #[test]
    fn focus_prev_field_retreats_from_title_to_submit() {
        use crate::state::TaskFormField;
        let mut app = modal_app();
        app.update(Action::FocusPrevField);
        assert_eq!(
            app.ui.modal.as_ref().unwrap().focused_field,
            TaskFormField::Submit
        );
    }

    #[test]
    fn cycle_task_kind_advances_from_review_to_implement() {
        let mut app = modal_app();
        // blank() defaults to Implement; set to Review to test the cycle start
        app.ui.modal.as_mut().unwrap().kind = domain::TaskKind::Review;
        app.update(Action::CycleTaskKind);
        assert_eq!(
            app.ui.modal.as_ref().unwrap().kind,
            domain::TaskKind::Implement
        );
    }

    #[test]
    fn commit_task_creation_with_blank_title_flashes_error_and_keeps_modal() {
        let mut app = modal_app();
        let effects = app.update(Action::CommitTaskCreation);
        assert!(effects.is_empty());
        assert_eq!(app.ui.flash.as_deref(), Some("Title required"));
        assert!(app.ui.modal.is_some());
    }

    #[test]
    fn commit_task_creation_with_title_emits_create_task_and_clears_modal() {
        use crate::state::TaskCreationModal;
        use tui_textarea::TextArea;
        let mut modal = TaskCreationModal::blank();
        modal.title = TextArea::new(vec!["Fix auth bug".to_string()]);
        let mut app = App {
            ui: UiState {
                modal: Some(modal),
                ..UiState::default()
            },
            ..App::default()
        };
        let effects = app.update(Action::CommitTaskCreation);
        assert!(app.ui.modal.is_none());
        assert_eq!(effects.len(), 1);
        let Effect::CreateTask {
            title,
            description,
            kind,
            links,
        } = effects.into_iter().next().unwrap()
        else {
            panic!("expected CreateTask");
        };
        assert_eq!(title, "Fix auth bug");
        assert!(description.is_none());
        assert_eq!(kind, domain::TaskKind::Implement);
        assert!(links.is_empty());
    }

    #[test]
    fn commit_task_creation_with_all_fields_populates_effect_correctly() {
        use crate::state::TaskCreationModal;
        use tui_textarea::TextArea;
        let mut modal = TaskCreationModal::blank();
        modal.title = TextArea::new(vec!["Fix auth bug".to_string()]);
        modal.description = TextArea::new(vec!["Auth is broken".to_string()]);
        modal.kind = domain::TaskKind::Debug;
        modal.link = TextArea::new(vec!["https://github.com/org/repo/issues/1".to_string()]);
        let mut app = App {
            ui: UiState {
                modal: Some(modal),
                ..UiState::default()
            },
            ..App::default()
        };
        let effects = app.update(Action::CommitTaskCreation);
        let Effect::CreateTask {
            title,
            description,
            kind,
            links,
        } = effects.into_iter().next().unwrap()
        else {
            panic!("expected CreateTask");
        };
        assert_eq!(title, "Fix auth bug");
        assert_eq!(description.as_deref(), Some("Auth is broken"));
        assert_eq!(kind, domain::TaskKind::Debug);
        assert_eq!(links, vec!["https://github.com/org/repo/issues/1"]);
    }

    // --- OpenInOcto / OpenInLazygit from split view ---

    #[test]
    fn open_in_octo_from_split_view_emits_effect_with_pr_identity() {
        let mut app = split_view_app_with_pr_for_merge();
        app.ui.pending_pr_action = true;
        let effects = app.update(Action::OpenInOcto);
        assert_eq!(effects.len(), 1);
        let Effect::OpenInOcto {
            repo,
            number,
            head_branch,
        } = effects.into_iter().next().unwrap()
        else {
            panic!("expected OpenInOcto");
        };
        assert_eq!(repo, "ooloth/hub");
        assert_eq!(number, 7);
        assert_eq!(head_branch, "feat/thing");
    }

    #[test]
    fn open_in_lazygit_from_split_view_emits_effect_with_pr_identity() {
        let mut app = split_view_app_with_pr_for_merge();
        app.ui.pending_pr_action = true;
        let effects = app.update(Action::OpenInLazygit);
        assert_eq!(effects.len(), 1);
        let Effect::OpenInLazygit {
            repo,
            number,
            head_branch,
        } = effects.into_iter().next().unwrap()
        else {
            panic!("expected OpenInLazygit");
        };
        assert_eq!(repo, "ooloth/hub");
        assert_eq!(number, 7);
        assert_eq!(head_branch, "feat/thing");
    }

    // --- OpenReviewPicker from split view ---

    fn split_view_app_with_pr_item() -> App {
        use crate::display::{flatten, DisplayItem};
        let item = stub_pr();
        let items = vec![DisplayItem::Single(item)];
        let expanded = HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    flat_rows,
                    items,
                    selected: 0,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                    detail_mode: DetailMode::Visible { detail_scroll: 0 },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    #[test]
    fn open_review_picker_from_split_view_sets_pending_flag_and_stays_on_screen() {
        let mut app = split_view_app_with_pr_item();
        app.update(Action::OpenReviewPicker);
        assert!(app.ui.pending_review_action);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    #[test]
    fn commit_review_converge_from_split_view_emits_review_pr_effect() {
        let mut app = split_view_app_with_pr_item();
        app.ui.pending_review_action = true;
        let effects = app.update(Action::CommitReview(crate::state::ReviewSkill::Converge));
        assert!(!app.ui.pending_review_action);
        assert_eq!(effects.len(), 1);
        let Effect::ReviewPr {
            skill, ownership, ..
        } = effects.into_iter().next().unwrap()
        else {
            panic!("expected ReviewPr");
        };
        assert_eq!(skill, crate::state::ReviewSkill::Converge);
        assert_eq!(ownership, crate::state::PrOwnership::Owned);
    }

    #[test]
    fn commit_review_pr_comments_from_split_view_emits_review_pr_effect() {
        let mut app = split_view_app_with_pr_item();
        app.ui.pending_review_action = true;
        let effects = app.update(Action::CommitReview(
            crate::state::ReviewSkill::PrCommentsConverge,
        ));
        assert!(!app.ui.pending_review_action);
        assert_eq!(effects.len(), 1);
        let Effect::ReviewPr { skill, .. } = effects.into_iter().next().unwrap() else {
            panic!("expected ReviewPr");
        };
        assert_eq!(skill, crate::state::ReviewSkill::PrCommentsConverge);
    }

    #[test]
    fn cancel_review_from_split_view_clears_flag_stays_on_screen() {
        let mut app = split_view_app_with_pr_item();
        app.ui.pending_review_action = true;
        let effects = app.update(Action::CancelReview);
        assert!(!app.ui.pending_review_action);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
        assert!(effects.is_empty());
    }

    // --- Split view state transitions ---

    fn app_in_split_view() -> App {
        app_with_items(vec![DisplayItem::Single(ci_failure())])
    }

    fn app_in_split_view_with_scroll(detail_scroll: u16) -> App {
        let item = DisplayItem::Single(ci_failure());
        let expanded = HashSet::new();
        let flat_rows = flatten(&[item.clone()], &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items: vec![item],
                    flat_rows,
                    selected: 0,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                    detail_mode: DetailMode::Visible { detail_scroll },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    // ST1: Enter from Hidden activates split view with scroll reset to 0.
    #[test]
    fn enter_from_hidden_activates_split_view() {
        let mut app = app_in_split_view();
        app.update(Action::Enter);
        match app.current_screen() {
            Screen::UnifiedList { detail_mode, .. } => {
                assert_eq!(detail_mode, &DetailMode::Visible { detail_scroll: 0 });
            }
            _ => panic!("expected UnifiedList"),
        }
    }

    // ST2: Enter while split view is already active is a no-op.
    #[test]
    fn enter_while_split_view_active_is_noop() {
        let mut app = app_in_split_view_with_scroll(3);
        app.update(Action::Enter);
        match app.current_screen() {
            Screen::UnifiedList { detail_mode, .. } => {
                assert_eq!(detail_mode, &DetailMode::Visible { detail_scroll: 3 });
            }
            _ => panic!("expected UnifiedList"),
        }
    }

    // ST3: Back from Visible collapses to Hidden.
    #[test]
    fn back_from_split_view_collapses_to_fullscreen_list() {
        let mut app = app_in_split_view_with_scroll(2);
        app.update(Action::Back);
        match app.current_screen() {
            Screen::UnifiedList { detail_mode, .. } => {
                assert_eq!(detail_mode, &DetailMode::Hidden);
            }
            _ => panic!("expected UnifiedList"),
        }
    }

    // ST4: ClearFilter while Hidden with active filter clears filter, detail_mode stays Hidden.
    #[test]
    fn clear_filter_while_hidden_clears_filter_and_leaves_detail_hidden() {
        let item = DisplayItem::Single(ci_failure());
        let expanded = HashSet::new();
        let flat_rows = flatten(&[item.clone()], &expanded);
        let mut app = App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items: vec![item],
                    flat_rows,
                    selected: 0,
                    filter: Filter {
                        category: Some(Category::Prs),
                        query: None,
                    },
                    expanded_groups: expanded,
                    detail_mode: DetailMode::Hidden,
                },
                ..UiState::default()
            },
            ..App::default()
        };
        app.update(Action::ClearFilter);
        match app.current_screen() {
            Screen::UnifiedList {
                filter,
                detail_mode,
                ..
            } => {
                assert!(filter.is_empty());
                assert_eq!(detail_mode, &DetailMode::Hidden);
            }
            _ => panic!("expected UnifiedList"),
        }
    }

    // ST5: List navigation while Visible resets detail_scroll to 0.
    #[test]
    fn move_down_while_split_view_active_resets_detail_scroll() {
        let items = vec![
            DisplayItem::Single(ci_failure()),
            DisplayItem::Single(ci_failure()),
        ];
        let expanded = HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        let mut app = App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items,
                    flat_rows,
                    selected: 0,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                    detail_mode: DetailMode::Visible { detail_scroll: 7 },
                },
                ..UiState::default()
            },
            ..App::default()
        };
        app.update(Action::MoveDown);
        match app.current_screen() {
            Screen::UnifiedList {
                detail_mode,
                selected,
                ..
            } => {
                assert_eq!(*selected, 1);
                assert_eq!(detail_mode, &DetailMode::Visible { detail_scroll: 0 });
            }
            _ => panic!("expected UnifiedList"),
        }
    }

    // ST6: ScrollDetailDown while Visible increments detail_scroll.
    #[test]
    fn scroll_detail_down_while_split_view_active_increments_scroll() {
        let mut app = app_in_split_view_with_scroll(0);
        app.update(Action::ScrollDetailDown);
        match app.current_screen() {
            Screen::UnifiedList { detail_mode, .. } => {
                assert_eq!(detail_mode, &DetailMode::Visible { detail_scroll: 1 });
            }
            _ => panic!("expected UnifiedList"),
        }
    }

    // ST7: ScrollDetailDown while Hidden is a no-op (no panic, no state change).
    #[test]
    fn scroll_detail_down_while_hidden_is_noop() {
        let mut app = app_in_split_view();
        app.update(Action::ScrollDetailDown);
        match app.current_screen() {
            Screen::UnifiedList { detail_mode, .. } => {
                assert_eq!(detail_mode, &DetailMode::Hidden);
            }
            _ => panic!("expected UnifiedList"),
        }
    }

    // ST8: ScrollDetailUp while Visible decrements detail_scroll (saturating at 0).
    #[test]
    fn scroll_detail_up_while_split_view_active_decrements_scroll() {
        let mut app = app_in_split_view_with_scroll(3);
        app.update(Action::ScrollDetailUp);
        match app.current_screen() {
            Screen::UnifiedList { detail_mode, .. } => {
                assert_eq!(detail_mode, &DetailMode::Visible { detail_scroll: 2 });
            }
            _ => panic!("expected UnifiedList"),
        }
    }

    // --- OpenUrl ---

    fn pr_item() -> StatusItem {
        StatusItem::Pr(domain::PullRequest {
            number: 1,
            title: "Fix".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/1".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            kind: domain::PrKind::Mine,
            author: "ooloth".to_string(),
            review_decision: None,
            approval_count: 0,
            comment_count: 0,
            head_branch: "feat/fix".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: None,
        })
    }

    // U1: OpenUrl on an item with a URL emits Effect::OpenUrl with the item's URL.
    #[test]
    fn open_url_on_item_with_url_emits_open_url_effect() {
        let mut app = app_with_items(vec![DisplayItem::Single(pr_item())]);
        let effects = app.update(Action::OpenUrl);
        assert!(matches!(
            effects.as_slice(),
            [Effect::OpenUrl(u)] if u == "https://github.com/owner/repo/pull/1"
        ));
    }

    // U2: OpenUrl with no selected item (empty list) is a no-op.
    #[test]
    fn open_url_with_no_selection_is_noop() {
        let mut app = app_with_items(vec![]);
        let effects = app.update(Action::OpenUrl);
        assert!(effects.is_empty());
    }
}
