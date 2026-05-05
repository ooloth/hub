use chrono::{DateTime, Utc};
use ratatui::widgets::ListState;
use workflows::status::StatusItem;

use crate::display::{item_url, CatData, Category, DisplayItem};

pub(crate) const TILE_COLS: usize = 2;

#[derive(Debug)]
pub(crate) struct App {
    pub(crate) cats: Vec<CatData>,
    pub(crate) focused_tile: usize,
    pub(crate) view: View,
    pub(crate) is_refreshing: bool,
    pub(crate) last_updated: Option<DateTime<Utc>>,
    pub(crate) error: Option<String>,
    pub(crate) show_help: bool,
    pub(crate) flash: Option<String>,
}

#[derive(Debug)]
pub(crate) enum View {
    Home,
    Category {
        cat: Category,
        list_state: ListState,
    },
    Detail {
        cat: Category,
        group_index: usize,
        list_state: ListState,
    },
}

impl App {
    pub(crate) fn active_list_len(&self) -> usize {
        match &self.view {
            View::Home => self.cats.len(),
            View::Category { cat, .. } => self
                .cats
                .iter()
                .find(|c| c.cat == *cat)
                .map(|c| c.items.len())
                .unwrap_or(0),
            View::Detail {
                cat, group_index, ..
            } => self
                .cats
                .iter()
                .find(|c| c.cat == *cat)
                .and_then(|c| c.items.get(*group_index))
                .map(|d| match d {
                    DisplayItem::Group { items, .. } => items.len(),
                    _ => 0,
                })
                .unwrap_or(0),
        }
    }

    pub(crate) fn move_tile_forward(&mut self) {
        let len = self.cats.len();
        if len > 0 {
            self.focused_tile = (self.focused_tile + 1) % len;
        }
    }

    pub(crate) fn move_tile_back(&mut self) {
        let len = self.cats.len();
        if len > 0 {
            self.focused_tile = (self.focused_tile + len - 1) % len;
        }
    }

    pub(crate) fn move_tile_up(&mut self) {
        let len = self.cats.len();
        if len == 0 {
            return;
        }
        let row = self.focused_tile / TILE_COLS;
        let col = self.focused_tile % TILE_COLS;
        if row > 0 {
            let target = (row - 1) * TILE_COLS + col;
            if target < len {
                self.focused_tile = target;
                return;
            }
        }
        self.focused_tile = (self.focused_tile + len - 1) % len;
    }

    pub(crate) fn move_tile_down(&mut self) {
        let len = self.cats.len();
        if len == 0 {
            return;
        }
        let row = self.focused_tile / TILE_COLS;
        let col = self.focused_tile % TILE_COLS;
        let target = (row + 1) * TILE_COLS + col;
        if target < len {
            self.focused_tile = target;
        } else {
            self.focused_tile = (self.focused_tile + 1) % len;
        }
    }

    pub(crate) fn move_tile_left(&mut self) {
        let len = self.cats.len();
        if len == 0 {
            return;
        }
        let col = self.focused_tile % TILE_COLS;
        if col > 0 {
            self.focused_tile -= 1;
        } else {
            self.focused_tile = (self.focused_tile + len - 1) % len;
        }
    }

    pub(crate) fn move_tile_right(&mut self) {
        let len = self.cats.len();
        if len == 0 {
            return;
        }
        let col = self.focused_tile % TILE_COLS;
        if col + 1 < TILE_COLS && self.focused_tile + 1 < len {
            self.focused_tile += 1;
        } else {
            self.focused_tile = (self.focused_tile + 1) % len;
        }
    }

    pub(crate) fn move_up(&mut self) {
        match &mut self.view {
            View::Home => {}
            View::Category { list_state, .. } | View::Detail { list_state, .. } => {
                let sel = list_state.selected().unwrap_or(0);
                if sel > 0 {
                    list_state.select(Some(sel - 1));
                }
            }
        }
    }

    pub(crate) fn move_down(&mut self) {
        let len = self.active_list_len();
        match &mut self.view {
            View::Home => {}
            View::Category { list_state, .. } | View::Detail { list_state, .. } => {
                let sel = list_state.selected().unwrap_or(0);
                if len > 0 && sel < len - 1 {
                    list_state.select(Some(sel + 1));
                }
            }
        }
    }

    pub(crate) fn selected_url(&self) -> Option<&str> {
        match &self.view {
            View::Home => None,
            View::Category { cat, list_state } => {
                let sel = list_state.selected().unwrap_or(0);
                let cd = self.cats.iter().find(|c| c.cat == *cat)?;
                match cd.items.get(sel)? {
                    DisplayItem::Single(item) => item_url(item),
                    DisplayItem::Group { .. } => None,
                }
            }
            View::Detail {
                cat,
                group_index,
                list_state,
            } => {
                let sel = list_state.selected().unwrap_or(0);
                let cd = self.cats.iter().find(|c| c.cat == *cat)?;
                match cd.items.get(*group_index)? {
                    DisplayItem::Group { items, .. } => items.get(sel).and_then(item_url),
                    _ => None,
                }
            }
        }
    }
}

pub(crate) enum EnterAction {
    None,
    OpenUrl(String),
    OpenCategory {
        cat: Category,
    },
    OpenDetail {
        cat: Category,
        group_index: usize,
        item_count: usize,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum InvestigateAction {
    None,
    LaunchCi { repo: String, run_url: String },
}

pub(crate) fn compute_enter_action(app: &App) -> EnterAction {
    match &app.view {
        View::Home => {
            if app.cats.is_empty() {
                return EnterAction::None;
            }
            EnterAction::OpenCategory {
                cat: app.cats[app.focused_tile].cat,
            }
        }
        View::Category { cat, list_state } => {
            let sel = list_state.selected().unwrap_or(0);
            let Some(cd) = app.cats.iter().find(|c| c.cat == *cat) else {
                return EnterAction::None;
            };
            match cd.items.get(sel) {
                Some(DisplayItem::Group { items, .. }) => EnterAction::OpenDetail {
                    cat: *cat,
                    group_index: sel,
                    item_count: items.len(),
                },
                Some(DisplayItem::Single(_)) => app
                    .selected_url()
                    .map(|u| EnterAction::OpenUrl(u.to_string()))
                    .unwrap_or(EnterAction::None),
                None => EnterAction::None,
            }
        }
        View::Detail { .. } => app
            .selected_url()
            .map(|u| EnterAction::OpenUrl(u.to_string()))
            .unwrap_or(EnterAction::None),
    }
}

pub(crate) fn compute_investigate_action(app: &App) -> InvestigateAction {
    let item = match &app.view {
        View::Home => return InvestigateAction::None,
        View::Category { cat, list_state } => {
            let sel = list_state.selected().unwrap_or(0);
            let Some(cd) = app.cats.iter().find(|c| c.cat == *cat) else {
                return InvestigateAction::None;
            };
            let Some(display_item) = cd.items.get(sel) else {
                return InvestigateAction::None;
            };
            match display_item {
                DisplayItem::Single(item) => item,
                DisplayItem::Group { .. } => return InvestigateAction::None,
            }
        }
        View::Detail {
            cat,
            group_index,
            list_state,
        } => {
            let sel = list_state.selected().unwrap_or(0);
            let Some(cd) = app.cats.iter().find(|c| c.cat == *cat) else {
                return InvestigateAction::None;
            };
            let Some(display_item) = cd.items.get(*group_index) else {
                return InvestigateAction::None;
            };
            match display_item {
                DisplayItem::Group { items, .. } => {
                    let Some(item) = items.get(sel) else {
                        return InvestigateAction::None;
                    };
                    item
                }
                _ => return InvestigateAction::None,
            }
        }
    };
    match item {
        StatusItem::Ci(c) => InvestigateAction::LaunchCi {
            repo: c.repo.to_string(),
            run_url: c.url.clone(),
        },
        _ => InvestigateAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::{compute_investigate_action, App, Category, InvestigateAction, View};
    use crate::display::{CatData, DisplayItem};
    use ratatui::widgets::ListState;
    use workflows::status::StatusItem;

    #[test]
    fn investigate_action_launches_ci_from_category_selection() {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let app = App {
            cats: vec![CatData {
                cat: Category::Errors,
                items: vec![DisplayItem::Single(ci_failure())],
            }],
            focused_tile: 0,
            view: View::Category {
                cat: Category::Errors,
                list_state,
            },
            is_refreshing: false,
            last_updated: None,
            error: None,
            show_help: false,
            flash: None,
        };

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
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let app = App {
            cats: vec![CatData {
                cat: Category::Errors,
                items: vec![DisplayItem::Group {
                    label: "group".to_string(),
                    items: vec![ci_failure()],
                }],
            }],
            focused_tile: 0,
            view: View::Detail {
                cat: Category::Errors,
                group_index: 0,
                list_state,
            },
            is_refreshing: false,
            last_updated: None,
            error: None,
            show_help: false,
            flash: None,
        };

        assert_eq!(
            compute_investigate_action(&app),
            InvestigateAction::LaunchCi {
                repo: "ooloth/hub".to_string(),
                run_url: "https://github.com/ooloth/hub/actions/runs/123".to_string(),
            }
        );
    }

    #[test]
    fn investigate_action_ignores_unmapped_items() {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let app = App {
            cats: vec![CatData {
                cat: Category::Issues,
                items: vec![DisplayItem::Single(StatusItem::Issue(domain::Issue {
                    number: 31,
                    title: "TUI investigation".to_string(),
                    repo: domain::RepoSlug::new("ooloth", "hub"),
                    url: "https://github.com/ooloth/hub/issues/31".to_string(),
                    age: chrono::Duration::zero(),
                    urgency: domain::Urgency::Low,
                    labels: vec![],
                }))],
            }],
            focused_tile: 0,
            view: View::Category {
                cat: Category::Issues,
                list_state,
            },
            is_refreshing: false,
            last_updated: None,
            error: None,
            show_help: false,
            flash: None,
        };

        assert_eq!(compute_investigate_action(&app), InvestigateAction::None);
    }

    fn ci_failure() -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("ooloth", "hub"),
            workflow_name: "CI".to_string(),
            conclusion: "failure".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/ooloth/hub/actions/runs/123".to_string(),
        })
    }
}
