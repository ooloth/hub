use anyhow::{Context, Result};
use chrono::Utc;

use domain::{agent_ready_labels, dismissed_labels};

use super::{
    Action, App, DetailMode, Effect, InvestigateAction, Msg, PrOwnership, PrPrevScreen,
    RefreshState, Screen,
};
use crate::display::{
    build_unified, flatten, item_investigation, item_url, lines_to_compact_json, Filter, FlatRow,
    InvestigationKind, ListSnapshot, LogDetailView,
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
                self.ui.screen = match std::mem::take(&mut self.ui.screen) {
                    Screen::LogDetail { parent, .. }
                    | Screen::IssueDetail { parent, .. }
                    | Screen::PrDetail { parent, .. } => {
                        let flat_rows = flatten(&parent.items, &parent.expanded_groups);
                        Screen::UnifiedList {
                            items: parent.items,
                            flat_rows,
                            selected: parent.selected,
                            filter: parent.filter,
                            expanded_groups: parent.expanded_groups,
                            detail_mode: DetailMode::Hidden,
                        }
                    }
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
            | Action::DismissIssue
            | Action::DismissInput(_)
            | Action::CommitDismissal
            | Action::CancelDismissal
            | Action::Investigate
            | Action::AskAboutPr
            | Action::OpenInOcto
            | Action::OpenInLazygit
            | Action::OpenReviewPicker
            | Action::CommitReview(_)
            | Action::CancelReview => match &self.ui.screen {
                Screen::UnifiedList { .. } => self.handle_unified_list(action),
                Screen::LogDetail { .. } => self.handle_log_detail(action),
                Screen::IssueDetail { .. } => self.handle_issue_reader(action),
                Screen::PrDetail { .. } => self.handle_pr_reader(action),
                Screen::MergingPr { .. } => self.handle_merging_pr(action),
                Screen::DismissingIssue { .. } => self.handle_dismissing(action),
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
            Action::DismissIssue => {
                let Some(StatusItem::Issue(issue)) = self.ui.screen.selected_status_item() else {
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
                self.ui.screen = Screen::DismissingIssue {
                    parent: snapshot,
                    issue,
                    input: tui_input::Input::default(),
                };
                vec![]
            }
            Action::Investigate => self.handle_investigate(),
            _ => unreachable!(),
        }
    }

    fn handle_log_detail(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::MoveUp => {
                if let Screen::LogDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = scroll.saturating_sub(1);
                }
                vec![]
            }
            Action::MoveDown => {
                if let Screen::LogDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = scroll.saturating_add(1);
                }
                vec![]
            }
            Action::MoveToTop => {
                if let Screen::LogDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = 0;
                }
                vec![]
            }
            Action::MoveToBottom => {
                if let Screen::LogDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = u16::MAX;
                }
                vec![]
            }
            Action::MovePageUp => {
                if let Screen::LogDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = scroll.saturating_sub(10);
                }
                vec![]
            }
            Action::MovePageDown => {
                if let Screen::LogDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = scroll.saturating_add(10);
                }
                vec![]
            }
            Action::Enter => self
                .selected_url()
                .map(|u| vec![Effect::OpenUrl(u.to_string())])
                .unwrap_or_default(),
            Action::Investigate => self.handle_investigate(),
            _ => unreachable!(),
        }
    }

    fn handle_issue_reader(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::MoveUp => {
                if let Screen::IssueDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = scroll.saturating_sub(1);
                }
                vec![]
            }
            Action::MoveDown => {
                if let Screen::IssueDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = scroll.saturating_add(1);
                }
                vec![]
            }
            Action::MoveToTop => {
                if let Screen::IssueDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = 0;
                }
                vec![]
            }
            Action::MoveToBottom => {
                // max scroll is clamped in render; set to a large value and let render clamp it
                if let Screen::IssueDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = u16::MAX;
                }
                vec![]
            }
            Action::MovePageUp => {
                if let Screen::IssueDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = scroll.saturating_sub(10);
                }
                vec![]
            }
            Action::MovePageDown => {
                if let Screen::IssueDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = scroll.saturating_add(10);
                }
                vec![]
            }
            Action::Enter => self
                .selected_url()
                .map(|u| vec![Effect::OpenUrl(u.to_string())])
                .unwrap_or_default(),
            Action::ApproveForAgent => {
                let Screen::IssueDetail { issue, .. } = &self.ui.screen else {
                    return vec![];
                };
                let labels = agent_ready_labels(&issue.labels);
                vec![Effect::SetIssueLabels {
                    repo: issue.repo.to_string(),
                    number: issue.number,
                    labels,
                }]
            }
            Action::DismissIssue => {
                let Screen::IssueDetail { issue, parent, .. } = &self.ui.screen else {
                    return vec![];
                };
                let issue = issue.clone();
                let parent = parent.clone();
                self.ui.screen = Screen::DismissingIssue {
                    parent,
                    issue,
                    input: tui_input::Input::default(),
                };
                vec![]
            }
            Action::Investigate => self.handle_investigate(),
            _ => unreachable!(),
        }
    }

    fn handle_pr_reader(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::MoveUp => {
                if let Screen::PrDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = scroll.saturating_sub(1);
                }
                vec![]
            }
            Action::MoveDown => {
                if let Screen::PrDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = scroll.saturating_add(1);
                }
                vec![]
            }
            Action::MoveToTop => {
                if let Screen::PrDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = 0;
                }
                vec![]
            }
            Action::MoveToBottom => {
                if let Screen::PrDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = u16::MAX;
                }
                vec![]
            }
            Action::MovePageUp => {
                if let Screen::PrDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = scroll.saturating_sub(10);
                }
                vec![]
            }
            Action::MovePageDown => {
                if let Screen::PrDetail { scroll, .. } = &mut self.ui.screen {
                    *scroll = scroll.saturating_add(10);
                }
                vec![]
            }
            Action::Enter => self
                .selected_url()
                .map(|u| vec![Effect::OpenUrl(u.to_string())])
                .unwrap_or_default(),
            Action::MergePr => {
                let Screen::PrDetail { pr, parent, .. } = &self.ui.screen else {
                    return vec![];
                };
                let pr = pr.clone();
                let parent = parent.clone();
                self.ui.screen = Screen::MergingPr {
                    parent,
                    pr,
                    prev: PrPrevScreen::PrDetail,
                };
                vec![]
            }
            Action::AskAboutPr => {
                let Screen::PrDetail { pr, .. } = &self.ui.screen else {
                    return vec![];
                };
                let ownership = PrOwnership::from_kind(pr.kind);
                vec![Effect::AskAboutPr {
                    repo: pr.repo.to_string(),
                    number: pr.number,
                    ownership,
                    head_branch: pr.head_branch.clone(),
                }]
            }
            Action::OpenInOcto => {
                let Screen::PrDetail { pr, .. } = &self.ui.screen else {
                    return vec![];
                };
                vec![Effect::OpenInOcto {
                    repo: pr.repo.to_string(),
                    number: pr.number,
                    head_branch: pr.head_branch.clone(),
                }]
            }
            Action::OpenInLazygit => {
                let Screen::PrDetail { pr, .. } = &self.ui.screen else {
                    return vec![];
                };
                vec![Effect::OpenInLazygit {
                    repo: pr.repo.to_string(),
                    number: pr.number,
                    head_branch: pr.head_branch.clone(),
                }]
            }
            Action::Investigate => self.handle_investigate(),
            _ => unreachable!(),
        }
    }

    fn handle_merging_pr(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::CancelMerge => {
                let Screen::MergingPr { parent, pr, prev } = std::mem::take(&mut self.ui.screen)
                else {
                    return vec![];
                };
                self.ui.screen = restore_after_pr_action(parent, pr, prev);
                vec![]
            }
            Action::CommitMerge => {
                let Screen::MergingPr { parent, pr, prev } = std::mem::take(&mut self.ui.screen)
                else {
                    return vec![];
                };
                let repo = pr.repo.to_string();
                let number = pr.number;
                self.ui.screen = restore_after_pr_action(parent, pr, prev);
                vec![Effect::MergePullRequest { repo, number }]
            }
            _ => unreachable!(),
        }
    }

    fn handle_dismissing(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::CancelDismissal => {
                let Screen::DismissingIssue { parent, .. } = std::mem::take(&mut self.ui.screen)
                else {
                    return vec![];
                };
                self.ui.screen = Screen::UnifiedList {
                    flat_rows: flatten(&parent.items, &parent.expanded_groups),
                    items: parent.items,
                    selected: parent.selected,
                    filter: parent.filter,
                    expanded_groups: parent.expanded_groups,
                    detail_mode: parent.detail_mode,
                };
                vec![]
            }
            Action::CommitDismissal => {
                let Screen::DismissingIssue {
                    parent,
                    issue,
                    input,
                } = std::mem::take(&mut self.ui.screen)
                else {
                    return vec![];
                };
                let reason = input.value().to_string();
                let labels = dismissed_labels(&issue.labels);
                let repo = issue.repo.to_string();
                let number = issue.number;
                self.ui.screen = Screen::UnifiedList {
                    flat_rows: flatten(&parent.items, &parent.expanded_groups),
                    items: parent.items,
                    selected: parent.selected,
                    filter: parent.filter,
                    expanded_groups: parent.expanded_groups,
                    detail_mode: parent.detail_mode,
                };
                vec![Effect::DismissIssue {
                    repo,
                    number,
                    reason,
                    labels,
                }]
            }
            Action::DismissInput(req) => {
                if let Screen::DismissingIssue { input, .. } = &mut self.ui.screen {
                    input.handle(req);
                }
                vec![]
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
            Screen::LogDetail { view, .. } => {
                let url = match view {
                    LogDetailView::Gcp { url, .. } | LogDetailView::Loki { url, .. } => {
                        url.as_str()
                    }
                };
                if url.is_empty() {
                    None
                } else {
                    Some(url)
                }
            }
            Screen::IssueDetail { issue, .. } | Screen::DismissingIssue { issue, .. } => {
                Some(&issue.url)
            }
            Screen::PrDetail { pr, .. } | Screen::MergingPr { pr, .. } => Some(&pr.url),
        }
    }
}

fn restore_after_pr_action(
    parent: ListSnapshot,
    pr: domain::PullRequest,
    prev: PrPrevScreen,
) -> Screen {
    match prev {
        PrPrevScreen::PrDetail => Screen::PrDetail {
            parent,
            pr,
            scroll: 0,
        },
        PrPrevScreen::UnifiedList { snapshot } => Screen::UnifiedList {
            flat_rows: flatten(&snapshot.items, &snapshot.expanded_groups),
            items: snapshot.items,
            selected: snapshot.selected,
            filter: snapshot.filter,
            expanded_groups: snapshot.expanded_groups,
            detail_mode: snapshot.detail_mode,
        },
    }
}

pub(crate) fn compute_investigate_action(app: &App) -> InvestigateAction {
    // LogDetail carries the view directly — serialise all lines to a compact JSON array.
    if let Screen::LogDetail { view, .. } = &app.ui.screen {
        return match view {
            LogDetailView::Gcp {
                project,
                env,
                title,
                message,
                url,
                lookback,
                lines,
                gcp_project,
            } => InvestigateAction::LaunchGcp {
                project: project.clone(),
                env: env.clone(),
                title: title.clone(),
                message: message.clone(),
                line: lines_to_compact_json(lines),
                url: url.clone(),
                lookback: lookback.clone(),
                gcp_project: gcp_project.clone(),
            },
            LogDetailView::Loki {
                project,
                env,
                title,
                message,
                url,
                lookback,
                lines,
            } => InvestigateAction::LaunchLoki {
                project: project.clone(),
                env: env.clone(),
                title: title.clone(),
                message: message.clone(),
                line: lines_to_compact_json(lines),
                url: url.clone(),
                lookback: lookback.clone(),
            },
        };
    }

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
        Screen::LogDetail { parent, .. }
        | Screen::IssueDetail { parent, .. }
        | Screen::PrDetail { parent, .. }
        | Screen::MergingPr { parent, .. }
        | Screen::DismissingIssue { parent, .. } => {
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
    use crate::display::{
        flatten, log_detail_view_from_item, Category, DisplayItem, Filter, GroupKey, ListSnapshot,
        LogDetailView, LogLine,
    };
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

    fn app_in_log_detail(item: StatusItem) -> App {
        let view = log_detail_view_from_item(&item).unwrap_or_else(|| LogDetailView::Gcp {
            project: "p".to_string(),
            env: "e".to_string(),
            title: "t".to_string(),
            message: "m".to_string(),
            url: "https://example.com".to_string(),
            lookback: "1h".to_string(),
            lines: vec![LogLine::parse("{}")],
            gcp_project: String::new(),
        });
        let parent = ListSnapshot {
            items: vec![DisplayItem::Single(item)],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: HashSet::new(),
            detail_mode: crate::state::DetailMode::Hidden,
        };
        App {
            ui: UiState {
                screen: Screen::LogDetail {
                    parent,
                    view,
                    scroll: 0,
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
    fn investigate_action_launches_gcp_from_log_detail() {
        let gcp = StatusItem::Gcp(domain::GcpEntry {
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
        });
        let app = app_in_log_detail(gcp);
        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchGcp {
                project: "mapapp".to_string(),
                env: "neuro".to_string(),
                title: "errors".to_string(),
                message: "something broke".to_string(),
                line: r#"[{"message":"something broke"}]"#.to_string(),
                url: "https://console.cloud.google.com/logs/query".to_string(),
                lookback: "7d".to_string(),
                gcp_project: "mapapp-prod-abc123".to_string(),
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

    // --- IssueDetail: Enter on issue opens reader ---

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

    fn app_in_issue_detail() -> App {
        let parent = ListSnapshot {
            items: vec![DisplayItem::Single(stub_issue())],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: HashSet::new(),
            detail_mode: crate::state::DetailMode::Hidden,
        };
        let issue = match stub_issue() {
            StatusItem::Issue(i) => i,
            _ => unreachable!(),
        };
        App {
            ui: UiState {
                screen: Screen::IssueDetail {
                    parent,
                    issue,
                    scroll: 0,
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn issue_detail_scroll(app: &App) -> u16 {
        match app.current_screen() {
            Screen::IssueDetail { scroll, .. } => *scroll,
            _ => panic!("expected IssueDetail"),
        }
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

    // --- IssueDetail: scroll ---

    #[test]
    fn issue_detail_move_down_increments_scroll() {
        let mut app = app_in_issue_detail();
        app.update(Action::MoveDown);
        assert_eq!(issue_detail_scroll(&app), 1);
    }

    #[test]
    fn issue_detail_move_up_at_zero_is_noop() {
        let mut app = app_in_issue_detail();
        app.update(Action::MoveUp);
        assert_eq!(issue_detail_scroll(&app), 0);
    }

    #[test]
    fn issue_detail_move_up_decrements_scroll() {
        let mut app = app_in_issue_detail();
        apply(
            &mut app,
            &[Action::MoveDown, Action::MoveDown, Action::MoveUp],
        );
        assert_eq!(issue_detail_scroll(&app), 1);
    }

    #[test]
    fn issue_detail_move_to_top_resets_scroll_to_zero() {
        let mut app = app_in_issue_detail();
        apply(
            &mut app,
            &[Action::MoveDown, Action::MoveDown, Action::MoveToTop],
        );
        assert_eq!(issue_detail_scroll(&app), 0);
    }

    #[test]
    fn issue_detail_move_to_bottom_sets_max_scroll() {
        let mut app = app_in_issue_detail();
        app.update(Action::MoveToBottom);
        assert_eq!(issue_detail_scroll(&app), u16::MAX);
    }

    #[test]
    fn issue_detail_page_down_adds_10() {
        let mut app = app_in_issue_detail();
        app.update(Action::MovePageDown);
        assert_eq!(issue_detail_scroll(&app), 10);
    }

    #[test]
    fn issue_detail_page_up_subtracts_10() {
        let mut app = app_in_issue_detail();
        apply(&mut app, &[Action::MovePageDown, Action::MovePageDown]);
        assert_eq!(issue_detail_scroll(&app), 20);
        app.update(Action::MovePageUp);
        assert_eq!(issue_detail_scroll(&app), 10);
    }

    #[test]
    fn issue_detail_page_up_clamps_at_zero() {
        let mut app = app_in_issue_detail();
        apply(
            &mut app,
            &[Action::MovePageDown, Action::MovePageUp, Action::MovePageUp],
        );
        assert_eq!(issue_detail_scroll(&app), 0);
    }

    // --- IssueDetail: effects ---

    #[test]
    fn approve_for_agent_emits_set_issue_labels() {
        let mut app = app_in_issue_detail();
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

    #[test]
    fn enter_in_issue_detail_emits_open_url() {
        let mut app = app_in_issue_detail();
        let effects = app.update(Action::Enter);
        assert_eq!(effects.len(), 1);
        let Effect::OpenUrl(url) = effects.into_iter().next().unwrap() else {
            panic!("expected OpenUrl");
        };
        assert!(url.contains("issues/42"));
    }

    // --- DismissIssue flow ---

    fn app_in_dismissing() -> App {
        let mut app = app_in_issue_detail();
        app.update(Action::DismissIssue);
        app
    }

    #[test]
    fn dismiss_issue_transitions_to_dismissing_screen() {
        let app = app_in_dismissing();
        assert!(matches!(
            app.current_screen(),
            Screen::DismissingIssue { .. }
        ));
    }

    #[test]
    fn dismiss_issue_preserves_parent_and_issue() {
        let app = app_in_dismissing();
        let Screen::DismissingIssue { issue, .. } = app.current_screen() else {
            panic!("expected DismissingIssue");
        };
        assert_eq!(issue.number, 42);
        assert_eq!(issue.repo.to_string(), "ooloth/hub");
    }

    #[test]
    fn dismiss_input_insert_char_updates_value() {
        let mut app = app_in_dismissing();
        app.update(Action::DismissInput(tui_input::InputRequest::InsertChar(
            'h',
        )));
        app.update(Action::DismissInput(tui_input::InputRequest::InsertChar(
            'i',
        )));
        let Screen::DismissingIssue { input, .. } = app.current_screen() else {
            panic!("expected DismissingIssue");
        };
        assert_eq!(input.value(), "hi");
    }

    #[test]
    fn cancel_dismissal_returns_to_unified_list() {
        let mut app = app_in_dismissing();
        app.update(Action::CancelDismissal);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    #[test]
    fn cancel_dismissal_emits_no_effects() {
        let mut app = app_in_dismissing();
        let effects = app.update(Action::CancelDismissal);
        assert!(effects.is_empty());
    }

    #[test]
    fn commit_dismissal_returns_to_unified_list() {
        let mut app = app_in_dismissing();
        app.update(Action::CommitDismissal);
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
    }

    #[test]
    fn commit_dismissal_emits_dismiss_issue_effect_with_correct_fields() {
        let mut app = app_in_dismissing();
        app.update(Action::DismissInput(tui_input::InputRequest::InsertChar(
            'x',
        )));
        let effects = app.update(Action::CommitDismissal);
        assert_eq!(effects.len(), 1);
        let Effect::DismissIssue {
            repo,
            number,
            reason,
            labels,
        } = effects.into_iter().next().unwrap()
        else {
            panic!("expected DismissIssue");
        };
        assert_eq!(repo, "ooloth/hub");
        assert_eq!(number, 42);
        assert_eq!(reason, "x");
        assert!(labels.contains(&"wontfix".to_string()));
        assert!(!labels.contains(&"status:needs-human-review".to_string()));
    }

    #[test]
    fn commit_dismissal_with_empty_reason_still_emits_effect() {
        let mut app = app_in_dismissing();
        let effects = app.update(Action::CommitDismissal);
        assert_eq!(effects.len(), 1);
        let Effect::DismissIssue { reason, .. } = effects.into_iter().next().unwrap() else {
            panic!("expected DismissIssue");
        };
        assert_eq!(reason, "");
    }

    // --- helpers ---

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

    fn app_in_pr_detail() -> App {
        let parent = ListSnapshot {
            items: vec![DisplayItem::Single(stub_pr())],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: HashSet::new(),
            detail_mode: crate::state::DetailMode::Hidden,
        };
        let pr = match stub_pr() {
            StatusItem::Pr(p) => p,
            _ => unreachable!(),
        };
        App {
            ui: UiState {
                screen: Screen::PrDetail {
                    parent,
                    pr,
                    scroll: 0,
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    // --- IssueDetail refresh ---

    #[test]
    fn refresh_in_issue_detail_preserves_screen_variant() {
        let mut app = app_in_issue_detail();
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        assert!(matches!(app.current_screen(), Screen::IssueDetail { .. }));
    }

    #[test]
    fn refresh_in_issue_detail_preserves_scroll() {
        let mut app = app_in_issue_detail();
        apply(&mut app, &[Action::MovePageDown]); // scroll → 10
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        assert_eq!(issue_detail_scroll(&app), 10);
    }

    #[test]
    fn refresh_in_issue_detail_updates_parent_items() {
        let mut app = app_in_issue_detail(); // parent has stub_issue
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        let Screen::IssueDetail { parent, .. } = app.current_screen() else {
            panic!("expected IssueDetail");
        };
        // parent rebuilt from report: now contains the CI item, not the issue
        assert!(matches!(
            parent.items[0],
            DisplayItem::Single(workflows::status::StatusItem::Ci(_))
        ));
    }

    // --- PrDetail refresh ---

    fn pr_detail_scroll(app: &App) -> u16 {
        match app.current_screen() {
            Screen::PrDetail { scroll, .. } => *scroll,
            _ => panic!("expected PrDetail"),
        }
    }

    #[test]
    fn refresh_in_pr_detail_preserves_screen_variant() {
        let mut app = app_in_pr_detail();
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        assert!(matches!(app.current_screen(), Screen::PrDetail { .. }));
    }

    #[test]
    fn refresh_in_pr_detail_preserves_scroll() {
        let mut app = app_in_pr_detail();
        apply(&mut app, &[Action::MovePageDown]); // scroll → 10
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        assert_eq!(pr_detail_scroll(&app), 10);
    }

    #[test]
    fn refresh_in_pr_detail_updates_parent_items() {
        let mut app = app_in_pr_detail(); // parent has stub_pr
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        let Screen::PrDetail { parent, .. } = app.current_screen() else {
            panic!("expected PrDetail");
        };
        assert!(matches!(
            parent.items[0],
            DisplayItem::Single(workflows::status::StatusItem::Ci(_))
        ));
    }

    // --- DismissingIssue refresh ---

    #[test]
    fn refresh_in_dismissing_preserves_screen_variant() {
        let mut app = app_in_dismissing();
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        assert!(matches!(
            app.current_screen(),
            Screen::DismissingIssue { .. }
        ));
    }

    #[test]
    fn refresh_in_dismissing_preserves_input_value() {
        let mut app = app_in_dismissing();
        app.update(Action::DismissInput(tui_input::InputRequest::InsertChar(
            'x',
        )));
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        let Screen::DismissingIssue { input, .. } = app.current_screen() else {
            panic!("expected DismissingIssue");
        };
        assert_eq!(input.value(), "x");
    }

    #[test]
    fn refresh_in_dismissing_updates_parent_items() {
        let mut app = app_in_dismissing(); // parent has stub_issue
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        let Screen::DismissingIssue { parent, .. } = app.current_screen() else {
            panic!("expected DismissingIssue");
        };
        assert!(matches!(
            parent.items[0],
            DisplayItem::Single(workflows::status::StatusItem::Ci(_))
        ));
    }

    // --- MergePr flow ---

    fn app_in_merging() -> App {
        let mut app = app_in_pr_detail();
        app.update(Action::MergePr);
        app
    }

    #[test]
    fn merge_pr_transitions_to_merging_screen() {
        let app = app_in_merging();
        assert!(matches!(app.current_screen(), Screen::MergingPr { .. }));
    }

    #[test]
    fn merge_pr_preserves_parent_and_pr() {
        let app = app_in_merging();
        let Screen::MergingPr { pr, .. } = app.current_screen() else {
            panic!("expected MergingPr");
        };
        assert_eq!(pr.number, 7);
        assert_eq!(pr.repo.to_string(), "ooloth/hub");
    }

    #[test]
    fn cancel_merge_returns_to_pr_detail_with_same_pr() {
        let mut app = app_in_merging();
        app.update(Action::CancelMerge);
        let Screen::PrDetail { pr, .. } = app.current_screen() else {
            panic!("expected PrDetail");
        };
        assert_eq!(pr.number, 7);
    }

    #[test]
    fn cancel_merge_emits_no_effects() {
        let mut app = app_in_merging();
        let effects = app.update(Action::CancelMerge);
        assert!(effects.is_empty());
    }

    #[test]
    fn commit_merge_returns_to_pr_detail() {
        let mut app = app_in_merging();
        app.update(Action::CommitMerge);
        assert!(matches!(app.current_screen(), Screen::PrDetail { .. }));
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

    // --- AskAboutPr ---

    #[test]
    fn ask_about_pr_emits_ask_about_pr_effect_with_correct_ownership() {
        let mut app = app_in_pr_detail();
        let effects = app.update(Action::AskAboutPr);
        assert_eq!(effects.len(), 1);
        let Effect::AskAboutPr {
            repo,
            number,
            ownership,
            ..
        } = effects.into_iter().next().unwrap()
        else {
            panic!("expected AskAboutPr");
        };
        assert_eq!(repo, "ooloth/hub");
        assert_eq!(number, 7);
        assert_eq!(ownership, crate::state::PrOwnership::Owned);
    }

    // --- OpenInOcto ---

    #[test]
    fn open_in_octo_emits_open_in_octo_effect_with_pr_identity() {
        let mut app = app_in_pr_detail();
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

    // --- OpenInLazygit ---

    #[test]
    fn open_in_lazygit_emits_effect_with_pr_identity() {
        let mut app = app_in_pr_detail();
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
