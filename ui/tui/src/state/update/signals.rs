use domain::agent_ready_labels;

use crate::display::{
    build_unified, flatten, item_investigation, item_url, Filter, FlatRow, InvestigationKind,
    ListSnapshot,
};
use crate::state::{
    Action, App, DetailMode, Effect, InvestigateAction, PrOwnership, PrPrevScreen, Screen,
    SubmenuState,
};
use workflows::status::StatusItem;

impl App {
    pub(super) fn rebuild_unified(&mut self, new_filter: Filter) {
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

    pub(super) fn sync_query_to_filter(&mut self) {
        let query_text = self.ui.query_input.clone().filter(|q| !q.is_empty());
        if let Screen::UnifiedList { filter, .. } = &mut self.ui.screen {
            let cat = filter.category;
            self.rebuild_unified(Filter {
                category: cat,
                query: query_text,
            });
        }
    }

    pub(super) fn reset_detail_scroll(&mut self) {
        match &mut self.ui.screen {
            Screen::UnifiedList {
                detail_mode: DetailMode::Visible { detail_scroll },
                ..
            } => *detail_scroll = 0,
            // Moving away from a BadgedSignal row while the session is toggled
            // resets to signal-detail so the toggle doesn't persist on a
            // mismatched row.
            Screen::UnifiedList { detail_mode, .. }
                if matches!(detail_mode, DetailMode::VisibleSession { .. }) =>
            {
                *detail_mode = DetailMode::Visible { detail_scroll: 0 };
            }
            _ => {}
        }
    }

    pub(super) fn handle_unified_list(&mut self, action: Action) -> Vec<Effect> {
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
            Action::ToggleSessionDetail => {
                if let Screen::UnifiedList { detail_mode, .. } = &mut self.ui.screen {
                    *detail_mode = match detail_mode {
                        DetailMode::Visible { .. } => {
                            DetailMode::VisibleSession { detail_scroll: 0 }
                        }
                        DetailMode::VisibleSession { .. } => {
                            DetailMode::Visible { detail_scroll: 0 }
                        }
                        DetailMode::Hidden => DetailMode::Hidden,
                    };
                }
                vec![]
            }
            Action::ScrollDetailDown => {
                let detail_scroll = match &mut self.ui.screen {
                    Screen::UnifiedList {
                        detail_mode: DetailMode::Visible { detail_scroll },
                        ..
                    }
                    | Screen::UnifiedList {
                        detail_mode: DetailMode::VisibleSession { detail_scroll },
                        ..
                    } => Some(detail_scroll),
                    _ => None,
                };
                if let Some(scroll) = detail_scroll {
                    *scroll = scroll.saturating_add(1);
                }
                vec![]
            }
            Action::ScrollDetailUp => {
                let detail_scroll = match &mut self.ui.screen {
                    Screen::UnifiedList {
                        detail_mode: DetailMode::Visible { detail_scroll },
                        ..
                    }
                    | Screen::UnifiedList {
                        detail_mode: DetailMode::VisibleSession { detail_scroll },
                        ..
                    } => Some(detail_scroll),
                    _ => None,
                };
                if let Some(scroll) = detail_scroll {
                    *scroll = scroll.saturating_sub(1);
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
                self.ui.submenu = SubmenuState::ReviewPicker;
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

    pub(super) fn handle_investigate(&mut self) -> Vec<Effect> {
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

    pub(super) fn selected_url(&self) -> Option<&str> {
        match &self.ui.screen {
            Screen::UnifiedList {
                flat_rows,
                selected,
                ..
            } => match flat_rows.get(*selected)? {
                FlatRow::Single(item) => item_url(item),
                FlatRow::GroupChild { item, .. } => item_url(item),
                FlatRow::GroupHeader { .. } => None,
                FlatRow::BadgedSignal { item, .. } => item_url(item),
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
