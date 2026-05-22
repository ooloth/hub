use anyhow::{Context, Result};
use chrono::Utc;

use domain::{agent_ready_labels, dismissed_labels};

use super::{
    Action, App, Effect, EnterAction, InvestigateAction, Msg, PrOwnership, RefreshState, Screen,
};
use crate::display::{
    build_unified, flatten, item_investigation, item_url, lines_to_compact_json,
    log_detail_view_from_group, log_detail_view_from_item, DisplayItem, Filter, FlatRow,
    InvestigationKind, ListSnapshot, LogDetailView,
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
                    vec![Effect::StartRefresh]
                } else {
                    vec![]
                }
            }
            Action::Back => {
                self.ui.screen = match std::mem::take(&mut self.ui.screen) {
                    Screen::LogDetail { parent, .. }
                    | Screen::IssueDetail { parent, .. }
                    | Screen::PrDetail { parent, .. }
                    | Screen::ReviewingPr { parent, .. } => {
                        let flat_rows = flatten(&parent.items, &parent.expanded_groups);
                        Screen::UnifiedList {
                            items: parent.items,
                            flat_rows,
                            selected: parent.selected,
                            filter: parent.filter,
                            expanded_groups: parent.expanded_groups,
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
            | Action::OpenReviewPicker
            | Action::CommitReview(_)
            | Action::CancelReview => match &self.ui.screen {
                Screen::UnifiedList { .. } => self.handle_unified_list(action),
                Screen::LogDetail { .. } => self.handle_log_detail(action),
                Screen::IssueDetail { .. } => self.handle_issue_reader(action),
                Screen::PrDetail { .. } => self.handle_pr_reader(action),
                Screen::ReviewingPr { .. } => self.handle_reviewing_pr(action),
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
                self.ui.screen = Screen::MergingPr { parent, pr };
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
            Action::OpenReviewPicker => {
                let Screen::PrDetail { pr, parent, .. } = &self.ui.screen else {
                    return vec![];
                };
                let pr = pr.clone();
                let parent = parent.clone();
                self.ui.screen = Screen::ReviewingPr { parent, pr };
                vec![]
            }
            Action::Investigate => self.handle_investigate(),
            _ => unreachable!(),
        }
    }

    fn handle_reviewing_pr(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::CommitReview(skill) => {
                let Screen::ReviewingPr { pr, parent } = std::mem::take(&mut self.ui.screen) else {
                    return vec![];
                };
                let ownership = PrOwnership::from_kind(pr.kind);
                let repo = pr.repo.to_string();
                let number = pr.number;
                let head_branch = pr.head_branch.clone();
                self.ui.screen = Screen::PrDetail {
                    parent,
                    pr,
                    scroll: 0,
                };
                vec![Effect::ReviewPr {
                    repo,
                    number,
                    ownership,
                    skill,
                    head_branch,
                }]
            }
            Action::CancelReview => {
                let Screen::ReviewingPr { parent, pr } = std::mem::take(&mut self.ui.screen) else {
                    return vec![];
                };
                self.ui.screen = Screen::PrDetail {
                    parent,
                    pr,
                    scroll: 0,
                };
                vec![]
            }
            _ => unreachable!(),
        }
    }

    fn handle_merging_pr(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::CancelMerge => {
                let Screen::MergingPr { parent, pr } = std::mem::take(&mut self.ui.screen) else {
                    return vec![];
                };
                self.ui.screen = Screen::PrDetail {
                    parent,
                    pr,
                    scroll: 0,
                };
                vec![]
            }
            Action::CommitMerge => {
                let Screen::MergingPr { parent, pr } = std::mem::take(&mut self.ui.screen) else {
                    return vec![];
                };
                let repo = pr.repo.to_string();
                let number = pr.number;
                self.ui.screen = Screen::PrDetail {
                    parent,
                    pr,
                    scroll: 0,
                };
                vec![Effect::MergePullRequest { repo, number }]
            }
            _ => unreachable!(),
        }
    }

    fn handle_dismissing(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::CancelDismissal => {
                let Screen::DismissingIssue { parent, issue, .. } =
                    std::mem::take(&mut self.ui.screen)
                else {
                    return vec![];
                };
                self.ui.screen = Screen::IssueDetail {
                    parent,
                    issue,
                    scroll: 0,
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
                self.ui.screen = Screen::IssueDetail {
                    parent,
                    issue,
                    scroll: 0,
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
            } => vec![Effect::LaunchGcp {
                project,
                env,
                title,
                message,
                line,
                url,
                lookback,
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

    fn apply_enter_action(&mut self, ea: EnterAction) -> Vec<Effect> {
        match ea {
            EnterAction::None => vec![],
            EnterAction::OpenUrl(url) => vec![Effect::OpenUrl(url)],
            EnterAction::OpenLogDetail(view) => {
                let Screen::UnifiedList {
                    items,
                    selected,
                    filter,
                    expanded_groups,
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
                };
                self.ui.screen = Screen::LogDetail {
                    parent: snapshot,
                    view,
                    scroll: 0,
                };
                vec![]
            }
            EnterAction::OpenIssueDetail(issue) => {
                let Screen::UnifiedList {
                    items,
                    selected,
                    filter,
                    expanded_groups,
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
                };
                self.ui.screen = Screen::IssueDetail {
                    parent: snapshot,
                    issue,
                    scroll: 0,
                };
                vec![]
            }
            EnterAction::OpenPrDetail(pr) => {
                let Screen::UnifiedList {
                    items,
                    selected,
                    filter,
                    expanded_groups,
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
                };
                self.ui.screen = Screen::PrDetail {
                    parent: snapshot,
                    pr,
                    scroll: 0,
                };
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
            Screen::PrDetail { pr, .. }
            | Screen::ReviewingPr { pr, .. }
            | Screen::MergingPr { pr, .. } => Some(&pr.url),
        }
    }
}

pub(crate) fn compute_enter_action(app: &App) -> EnterAction {
    use workflows::status::StatusItem;
    match app.current_screen() {
        Screen::UnifiedList {
            flat_rows,
            selected,
            items,
            ..
        } => match flat_rows.get(*selected) {
            Some(FlatRow::GroupHeader { key, .. }) => {
                let group_items = items.iter().find_map(|di| match di {
                    DisplayItem::Group { label, items: gi } if label == key => Some(gi.as_slice()),
                    _ => None,
                });
                group_items
                    .and_then(log_detail_view_from_group)
                    .map(EnterAction::OpenLogDetail)
                    .unwrap_or(EnterAction::None)
            }
            Some(FlatRow::GroupChild { item, .. }) | Some(FlatRow::Single(item)) => match item {
                StatusItem::Issue(issue) => EnterAction::OpenIssueDetail(issue.clone()),
                StatusItem::Pr(pr) => EnterAction::OpenPrDetail(pr.clone()),
                StatusItem::Gcp(_) | StatusItem::Loki(_) => {
                    if let Some(view) = log_detail_view_from_item(item) {
                        EnterAction::OpenLogDetail(view)
                    } else {
                        EnterAction::None
                    }
                }
                _ => app
                    .selected_url()
                    .map(|u| EnterAction::OpenUrl(u.to_string()))
                    .unwrap_or(EnterAction::None),
            },
            None => EnterAction::None,
        },
        Screen::LogDetail { .. }
        | Screen::IssueDetail { .. }
        | Screen::DismissingIssue { .. }
        | Screen::PrDetail { .. }
        | Screen::ReviewingPr { .. }
        | Screen::MergingPr { .. } => app
            .selected_url()
            .map(|u| EnterAction::OpenUrl(u.to_string()))
            .unwrap_or(EnterAction::None),
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
            } => InvestigateAction::LaunchGcp {
                project: project.clone(),
                env: env.clone(),
                title: title.clone(),
                message: message.clone(),
                line: lines_to_compact_json(lines),
                url: url.clone(),
                lookback: lookback.clone(),
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
        }) => InvestigateAction::LaunchGcp {
            project,
            env,
            title,
            message,
            line,
            url,
            lookback,
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
        } => {
            let new_items = build_unified(raw.to_vec(), filter);
            *selected = (*selected).min(new_items.len().saturating_sub(1));
            *flat_rows = flatten(&new_items, expanded_groups);
            *items = new_items;
        }
        Screen::LogDetail { parent, .. }
        | Screen::IssueDetail { parent, .. }
        | Screen::PrDetail { parent, .. }
        | Screen::ReviewingPr { parent, .. }
        | Screen::MergingPr { parent, .. }
        | Screen::DismissingIssue { parent, .. } => {
            let new_items = build_unified(raw.to_vec(), &parent.filter);
            parent.selected = parent.selected.min(new_items.len().saturating_sub(1));
            parent.items = new_items;
        }
    }
}

fn apply_refresh(app: &mut App, report: workflows::status::StatusReport) -> Result<Vec<Effect>> {
    let json = serde_json::to_string(&report).context("failed to serialize status report")?;
    app.data.raw_items = report.items.clone();
    app.data.last_updated = Some(Utc::now());
    app.data.refresh_state = if report.errors.is_empty() {
        RefreshState::Idle
    } else {
        RefreshState::Partial(report.errors)
    };
    refresh_screen_in_place(&mut app.ui.screen, &app.data.raw_items);
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
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        compute_enter_action, compute_investigate_action, handle_msg, Action, App, Effect,
        EnterAction, InvestigateAction, Msg, RefreshState, Screen,
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
        });
        let parent = ListSnapshot {
            items: vec![DisplayItem::Single(item)],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: HashSet::new(),
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
    fn enter_on_gcp_group_header_opens_log_detail() {
        let mut app = app_with_items(vec![DisplayItem::Group {
            label: GroupKey::new("hub".to_string()),
            items: vec![gcp_item(), gcp_item()],
        }]);
        app.update(Action::Enter);
        assert!(matches!(app.current_screen(), Screen::LogDetail { .. }));
    }

    #[test]
    fn enter_on_gcp_group_header_includes_all_lines() {
        let mut app = app_with_items(vec![DisplayItem::Group {
            label: GroupKey::new("hub".to_string()),
            items: vec![gcp_item(), gcp_item()],
        }]);
        app.update(Action::Enter);
        match app.current_screen() {
            Screen::LogDetail { view, .. } => match view {
                LogDetailView::Gcp { lines, .. } => assert_eq!(lines.len(), 2),
                _ => panic!("expected Gcp view"),
            },
            _ => panic!("expected LogDetail"),
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
    fn enter_on_issue_in_unified_list_opens_issue_detail() {
        let mut app = app_with_items(vec![DisplayItem::Single(stub_issue())]);
        app.update(Action::Enter);
        assert!(matches!(app.current_screen(), Screen::IssueDetail { .. }));
    }

    #[test]
    fn enter_on_ci_in_unified_list_opens_url() {
        let mut app = app_with_items(vec![DisplayItem::Single(ci_failure())]);
        let effects = app.update(Action::Enter);
        assert!(matches!(effects.as_slice(), [Effect::OpenUrl(_)]));
        assert!(matches!(app.current_screen(), Screen::UnifiedList { .. }));
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
    fn cancel_dismissal_returns_to_issue_detail_with_same_issue() {
        let mut app = app_in_dismissing();
        app.update(Action::CancelDismissal);
        let Screen::IssueDetail { issue, .. } = app.current_screen() else {
            panic!("expected IssueDetail");
        };
        assert_eq!(issue.number, 42);
    }

    #[test]
    fn cancel_dismissal_emits_no_effects() {
        let mut app = app_in_dismissing();
        let effects = app.update(Action::CancelDismissal);
        assert!(effects.is_empty());
    }

    #[test]
    fn commit_dismissal_returns_to_issue_detail() {
        let mut app = app_in_dismissing();
        app.update(Action::CommitDismissal);
        assert!(matches!(app.current_screen(), Screen::IssueDetail { .. }));
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

    // --- compute_enter_action ---

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
        })
    }

    fn app_in_pr_detail() -> App {
        let parent = ListSnapshot {
            items: vec![DisplayItem::Single(stub_pr())],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: HashSet::new(),
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

    #[test]
    fn compute_enter_action_on_issue_returns_open_issue_detail() {
        let app = app_with_items(vec![DisplayItem::Single(stub_issue())]);
        assert!(matches!(
            compute_enter_action(&app),
            EnterAction::OpenIssueDetail(_)
        ));
    }

    #[test]
    fn compute_enter_action_in_issue_detail_returns_open_url() {
        let app = app_in_issue_detail();
        assert!(matches!(
            compute_enter_action(&app),
            EnterAction::OpenUrl(_)
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

    // --- OpenReviewPicker / ReviewingPr ---

    fn app_in_reviewing() -> App {
        let mut app = app_in_pr_detail();
        app.update(Action::OpenReviewPicker);
        app
    }

    #[test]
    fn open_review_picker_transitions_to_reviewing_pr_screen() {
        let app = app_in_reviewing();
        assert!(matches!(app.current_screen(), Screen::ReviewingPr { .. }));
    }

    #[test]
    fn open_review_picker_preserves_parent_and_pr() {
        let app = app_in_reviewing();
        let Screen::ReviewingPr { pr, .. } = app.current_screen() else {
            panic!("expected ReviewingPr");
        };
        assert_eq!(pr.number, 7);
        assert_eq!(pr.repo.to_string(), "ooloth/hub");
    }

    #[test]
    fn commit_review_converge_returns_to_pr_detail_and_emits_review_pr_effect() {
        let mut app = app_in_reviewing();
        let effects = app.update(Action::CommitReview(crate::state::ReviewSkill::Converge));
        assert!(matches!(app.current_screen(), Screen::PrDetail { .. }));
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
    fn commit_review_pr_comments_returns_to_pr_detail_and_emits_review_pr_effect() {
        let mut app = app_in_reviewing();
        let effects = app.update(Action::CommitReview(
            crate::state::ReviewSkill::PrCommentsConverge,
        ));
        assert!(matches!(app.current_screen(), Screen::PrDetail { .. }));
        assert_eq!(effects.len(), 1);
        let Effect::ReviewPr { skill, .. } = effects.into_iter().next().unwrap() else {
            panic!("expected ReviewPr");
        };
        assert_eq!(skill, crate::state::ReviewSkill::PrCommentsConverge);
    }

    #[test]
    fn cancel_review_returns_to_pr_detail_with_no_effects() {
        let mut app = app_in_reviewing();
        let effects = app.update(Action::CancelReview);
        assert!(matches!(app.current_screen(), Screen::PrDetail { .. }));
        assert!(effects.is_empty());
    }
}
