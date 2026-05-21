use chrono::{DateTime, Utc};
use workflows::status::StatusItem;

use crate::display::DisplayItem;

mod types;
mod update;

pub(crate) use types::{
    Action, DetailView, Effect, EnterAction, InvestigateAction, Msg, RefreshState, Screen,
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
            Screen::UnifiedList { items, .. } => items.len(),
            Screen::Detail { parent, view } => match parent.items.get(view.group_index) {
                Some(DisplayItem::Group { items, .. }) => items.len(),
                _ => 0,
            },
            Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::DismissingIssue { .. } => 0,
        }
    }

    pub(crate) fn move_up(&mut self) {
        match &mut self.ui.screen {
            Screen::UnifiedList { selected, .. } => {
                *selected = selected.saturating_sub(1);
            }
            Screen::Detail { view, .. } => {
                assert!(
                    view.list_state.selected().is_some(),
                    "list_state must be selected in Detail screen"
                );
                let sel = view.list_state.selected().unwrap_or(0);
                if sel > 0 {
                    view.list_state.select(Some(sel - 1));
                }
            }
            Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::DismissingIssue { .. } => {}
        }
    }

    pub(crate) fn move_down(&mut self) {
        let len = self.active_list_len();
        match &mut self.ui.screen {
            Screen::UnifiedList { selected, .. } => {
                if len > 0 && *selected < len - 1 {
                    *selected += 1;
                }
            }
            Screen::Detail { view, .. } => {
                assert!(
                    view.list_state.selected().is_some(),
                    "list_state must be selected in Detail screen"
                );
                let sel = view.list_state.selected().unwrap_or(0);
                if len > 0 && sel < len - 1 {
                    view.list_state.select(Some(sel + 1));
                }
            }
            Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::DismissingIssue { .. } => {}
        }
    }

    pub(crate) fn move_to_top(&mut self) {
        match &mut self.ui.screen {
            Screen::UnifiedList { selected, .. } => *selected = 0,
            Screen::Detail { view, .. } => view.list_state.select(Some(0)),
            Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::DismissingIssue { .. } => {}
        }
    }

    pub(crate) fn move_to_bottom(&mut self) {
        let len = self.active_list_len();
        if len == 0 {
            return;
        }
        match &mut self.ui.screen {
            Screen::UnifiedList { selected, .. } => *selected = len - 1,
            Screen::Detail { view, .. } => view.list_state.select(Some(len - 1)),
            Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::DismissingIssue { .. } => {}
        }
    }

    pub(crate) fn move_page_up(&mut self) {
        const PAGE: usize = 10;
        match &mut self.ui.screen {
            Screen::UnifiedList { selected, .. } => {
                *selected = selected.saturating_sub(PAGE);
            }
            Screen::Detail { view, .. } => {
                assert!(
                    view.list_state.selected().is_some(),
                    "list_state must be selected in Detail screen"
                );
                let sel = view.list_state.selected().unwrap_or(0);
                view.list_state.select(Some(sel.saturating_sub(PAGE)));
            }
            Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
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
            Screen::UnifiedList { selected, .. } => {
                *selected = (*selected + PAGE).min(len - 1);
            }
            Screen::Detail { view, .. } => {
                assert!(
                    view.list_state.selected().is_some(),
                    "list_state must be selected in Detail screen"
                );
                let sel = view.list_state.selected().unwrap_or(0);
                view.list_state.select(Some((sel + PAGE).min(len - 1)));
            }
            Screen::IssueDetail { .. }
            | Screen::PrDetail { .. }
            | Screen::DismissingIssue { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{App, DetailView, Screen, UiState};
    use crate::display::{DisplayItem, Filter, ListSnapshot};
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

    fn detail_app(item_count: usize, selected: usize) -> App {
        let items: Vec<StatusItem> = (0..item_count).map(|_| stub_item()).collect();
        App {
            ui: UiState {
                screen: Screen::Detail {
                    parent: ListSnapshot {
                        items: vec![DisplayItem::Group {
                            label: "group".to_string(),
                            items,
                        }],
                        selected: 0,
                        filter: Filter::default(),
                    },
                    view: DetailView {
                        group_index: 0,
                        list_state: {
                            let mut ls = ratatui::widgets::ListState::default();
                            ls.select(Some(selected));
                            ls
                        },
                    },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn detail_selected(app: &App) -> usize {
        match &app.ui.screen {
            Screen::Detail { view, .. } => view.list_state.selected().unwrap(),
            _ => panic!("not in detail screen"),
        }
    }

    #[rstest]
    #[case(3, 1, 0)]
    #[case(3, 0, 0)]
    fn detail_move_up(#[case] n: usize, #[case] start: usize, #[case] end: usize) {
        let mut app = detail_app(n, start);
        app.move_up();
        assert_eq!(detail_selected(&app), end);
    }

    #[rstest]
    #[case(3, 1, 2)]
    #[case(3, 2, 2)]
    fn detail_move_down(#[case] n: usize, #[case] start: usize, #[case] end: usize) {
        let mut app = detail_app(n, start);
        app.move_down();
        assert_eq!(detail_selected(&app), end);
    }

    #[rstest]
    #[case(20, 15, 5)]
    #[case(20, 5, 0)]
    fn detail_move_page_up(#[case] n: usize, #[case] start: usize, #[case] end: usize) {
        let mut app = detail_app(n, start);
        app.move_page_up();
        assert_eq!(detail_selected(&app), end);
    }

    #[rstest]
    #[case(25, 5, 15)]
    #[case(25, 18, 24)]
    fn detail_move_page_down(#[case] n: usize, #[case] start: usize, #[case] end: usize) {
        let mut app = detail_app(n, start);
        app.move_page_down();
        assert_eq!(detail_selected(&app), end);
    }
}
