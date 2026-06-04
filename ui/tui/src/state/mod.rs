use chrono::{DateTime, Utc};
use domain::StreamBlock;
use workflows::status::StatusItem;

mod types;
mod update;

pub(crate) use types::{
    Action, DetailMode, Effect, InvestigateAction, Msg, PrOwnership, PrPrevScreen, RefreshState,
    ReviewSkill, Screen,
};
pub(crate) use update::{compute_investigate_action, handle_msg};

#[derive(Debug, Default)]
pub(crate) struct UiState {
    pub(crate) screen: Screen,
    pub(crate) show_help: bool,
    pub(crate) flash: Option<String>,
    pub(crate) query_input: Option<String>,
    pub(crate) pending_g: bool,
    /// True while the PR actions submenu is showing (d was pressed on a PR in split view).
    /// The next keypress either executes a sub-action or cancels.
    pub(crate) pending_pr_action: bool,
    /// True while the review picker submenu is showing (v was pressed on a PR in split view).
    /// The next keypress either commits a review skill or cancels.
    pub(crate) pending_review_action: bool,
    /// True while the task status submenu is showing (s was pressed on a task in split view).
    /// The next keypress either transitions to a new status or cancels.
    pub(crate) pending_task_status: bool,
}

#[derive(Debug, Default)]
pub(crate) struct DataState {
    pub(crate) raw_items: Vec<StatusItem>,
    pub(crate) refresh_state: RefreshState,
    pub(crate) last_updated: Option<DateTime<Utc>>,
    pub(crate) stream_blocks: Vec<StreamBlock>,
    pub(crate) stream_session_id: Option<String>,
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
            Screen::MergingPr { .. } | Screen::DismissingIssue { .. } => 0,
        }
    }

    pub(crate) fn move_up(&mut self) {
        if let Screen::UnifiedList { selected, .. } = &mut self.ui.screen {
            *selected = selected.saturating_sub(1);
        }
    }

    pub(crate) fn move_down(&mut self) {
        let len = self.active_list_len();
        if let Screen::UnifiedList { selected, .. } = &mut self.ui.screen {
            if len > 0 && *selected < len - 1 {
                *selected += 1;
            }
        }
    }

    pub(crate) fn move_to_top(&mut self) {
        if let Screen::UnifiedList { selected, .. } = &mut self.ui.screen {
            *selected = 0;
        }
    }

    pub(crate) fn move_to_bottom(&mut self) {
        let len = self.active_list_len();
        if len == 0 {
            return;
        }
        if let Screen::UnifiedList { selected, .. } = &mut self.ui.screen {
            *selected = len - 1;
        }
    }

    pub(crate) fn move_page_up(&mut self) {
        const PAGE: usize = 10;
        if let Screen::UnifiedList { selected, .. } = &mut self.ui.screen {
            *selected = selected.saturating_sub(PAGE);
        }
    }

    pub(crate) fn move_page_down(&mut self) {
        const PAGE: usize = 10;
        let len = self.active_list_len();
        if len == 0 {
            return;
        }
        if let Screen::UnifiedList { selected, .. } = &mut self.ui.screen {
            *selected = (*selected + PAGE).min(len - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{App, DetailMode, Screen, UiState};
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
                    detail_mode: DetailMode::Hidden,
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
