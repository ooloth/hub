use chrono::{DateTime, Utc};
use workflows::status::StatusItem;

mod types;
mod update;

pub(crate) use types::{
    Action, Effect, EnterAction, InvestigateAction, Msg, PrOwnership, RefreshState, ReviewSkill,
    Screen,
};
pub(crate) use update::{compute_enter_action, compute_investigate_action, handle_msg};

#[derive(Debug, Default)]
pub(crate) struct UiState {
    pub(crate) screen: Screen,
    pub(crate) show_help: bool,
    pub(crate) flash: Option<String>,
    pub(crate) query_input: Option<String>,
    pub(crate) pending_g: bool,
}

#[derive(Debug, Default)]
pub(crate) struct DataState {
    pub(crate) raw_items: Vec<StatusItem>,
    pub(crate) refresh_state: RefreshState,
    pub(crate) last_updated: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
pub(crate) struct App {
    pub(crate) ui: UiState,
    pub(crate) data: DataState,
}

impl App {
    pub(crate) fn current_screen(&self) -> &Screen {
        &self.ui.screen
    }

    pub(crate) fn active_list_len(&self) -> usize {
        match &self.ui.screen {
            Screen::UnifiedList { flat_rows, .. } => flat_rows.len(),
            Screen::LogDetail { .. }
            | Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::ReviewingPr { .. }
            | Screen::MergingPr { .. }
            | Screen::DismissingIssue { .. } => 0,
            Screen::PrSplit { items, .. } => items.len(),
        }
    }

    pub(crate) fn move_up(&mut self) {
        match &mut self.ui.screen {
            Screen::UnifiedList { selected, .. } | Screen::PrSplit { selected, .. } => {
                *selected = selected.saturating_sub(1);
            }
            Screen::LogDetail { .. }
            | Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::ReviewingPr { .. }
            | Screen::MergingPr { .. }
            | Screen::DismissingIssue { .. } => {}
        }
    }

    pub(crate) fn move_down(&mut self) {
        let len = self.active_list_len();
        match &mut self.ui.screen {
            Screen::UnifiedList { selected, .. } | Screen::PrSplit { selected, .. } => {
                if len > 0 && *selected < len - 1 {
                    *selected += 1;
                }
            }
            Screen::LogDetail { .. }
            | Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::ReviewingPr { .. }
            | Screen::MergingPr { .. }
            | Screen::DismissingIssue { .. } => {}
        }
    }

    pub(crate) fn move_to_top(&mut self) {
        match &mut self.ui.screen {
            Screen::UnifiedList { selected, .. } | Screen::PrSplit { selected, .. } => {
                *selected = 0;
            }
            Screen::LogDetail { .. }
            | Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::ReviewingPr { .. }
            | Screen::MergingPr { .. }
            | Screen::DismissingIssue { .. } => {}
        }
    }

    pub(crate) fn move_to_bottom(&mut self) {
        let len = self.active_list_len();
        if len == 0 {
            return;
        }
        match &mut self.ui.screen {
            Screen::UnifiedList { selected, .. } | Screen::PrSplit { selected, .. } => {
                *selected = len - 1;
            }
            Screen::LogDetail { .. }
            | Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::ReviewingPr { .. }
            | Screen::MergingPr { .. }
            | Screen::DismissingIssue { .. } => {}
        }
    }

    pub(crate) fn move_page_up(&mut self) {
        const PAGE: usize = 10;
        match &mut self.ui.screen {
            Screen::UnifiedList { selected, .. } | Screen::PrSplit { selected, .. } => {
                *selected = selected.saturating_sub(PAGE);
            }
            Screen::LogDetail { .. }
            | Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::ReviewingPr { .. }
            | Screen::MergingPr { .. }
            | Screen::DismissingIssue { .. } => {}
        }
    }

    pub(crate) fn move_page_down(&mut self) {
        const PAGE: usize = 10;
        let len = self.active_list_len();
        if len == 0 {
            return;
        }
        match &mut self.ui.screen {
            Screen::UnifiedList { selected, .. } | Screen::PrSplit { selected, .. } => {
                *selected = (*selected + PAGE).min(len - 1);
            }
            Screen::LogDetail { .. }
            | Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::ReviewingPr { .. }
            | Screen::MergingPr { .. }
            | Screen::DismissingIssue { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{App, Screen, UiState};
    use crate::display::{flatten, Filter, FlatRow};
    use rstest::rstest;
    use workflows::status::StatusItem;

    fn stub_item() -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("ooloth", "hub"),
            workflow_name: "CI".to_string(),
            job_name: None,
            step_name: None,
            error: None,
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://example.com".to_string(),
        })
    }

    fn list_app(item_count: usize, selected: usize) -> App {
        let items: Vec<_> = (0..item_count)
            .map(|_| crate::display::DisplayItem::Single(stub_item()))
            .collect();
        let expanded = HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items,
                    flat_rows,
                    selected,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn list_selected(app: &App) -> usize {
        match &app.ui.screen {
            Screen::UnifiedList { selected, .. } => *selected,
            _ => panic!("not in list screen"),
        }
    }

    #[rstest]
    #[case(3, 1, 0)]
    #[case(3, 0, 0)]
    fn list_move_up(#[case] n: usize, #[case] start: usize, #[case] end: usize) {
        let mut app = list_app(n, start);
        app.move_up();
        assert_eq!(list_selected(&app), end);
    }

    #[rstest]
    #[case(3, 1, 2)]
    #[case(3, 2, 2)]
    fn list_move_down(#[case] n: usize, #[case] start: usize, #[case] end: usize) {
        let mut app = list_app(n, start);
        app.move_down();
        assert_eq!(list_selected(&app), end);
    }

    #[rstest]
    #[case(20, 15, 5)]
    #[case(20, 5, 0)]
    fn list_move_page_up(#[case] n: usize, #[case] start: usize, #[case] end: usize) {
        let mut app = list_app(n, start);
        app.move_page_up();
        assert_eq!(list_selected(&app), end);
    }

    #[rstest]
    #[case(25, 5, 15)]
    #[case(25, 18, 24)]
    fn list_move_page_down(#[case] n: usize, #[case] start: usize, #[case] end: usize) {
        let mut app = list_app(n, start);
        app.move_page_down();
        assert_eq!(list_selected(&app), end);
    }

    #[test]
    fn active_list_len_returns_flat_rows_count() {
        let app = list_app(5, 0);
        assert_eq!(app.active_list_len(), 5);
    }

    #[test]
    fn flat_rows_single_items_are_single_variant() {
        let app = list_app(3, 0);
        match &app.ui.screen {
            Screen::UnifiedList { flat_rows, .. } => {
                assert!(flat_rows.iter().all(|r| matches!(r, FlatRow::Single(_))));
            }
            _ => panic!(),
        }
    }
}
