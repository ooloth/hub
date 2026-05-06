use chrono::{DateTime, Utc};

use crate::display::{CatData, Category, DisplayItem};

mod types;
mod update;

pub(crate) use types::{
    Action, CategoryView, DetailView, Effect, EnterAction, InvestigateAction, Msg, RefreshState,
    Screen,
};
pub(crate) use update::{compute_enter_action, compute_investigate_action, handle_msg};

pub(crate) const TILE_COLS: usize = 2;

#[derive(Debug, Default)]
pub(crate) struct UiState {
    pub(crate) focused_tile: usize,
    pub(crate) screen: Screen,
    pub(crate) show_help: bool,
    pub(crate) flash: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct DataState {
    pub(crate) cats: Vec<CatData>,
    pub(crate) refresh_state: RefreshState,
    pub(crate) last_updated: Option<DateTime<Utc>>,
}

impl DataState {
    pub(crate) fn cat_data(&self, cat: Category) -> Option<&CatData> {
        self.cats.iter().find(|c| c.cat == cat)
    }
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
            Screen::Home => self.data.cats.len(),
            Screen::Category(view) => self
                .data
                .cat_data(view.cat)
                .map(|c| c.items.len())
                .unwrap_or(0),
            Screen::Detail { view, .. } => self
                .data
                .cat_data(view.cat)
                .and_then(|c| c.items.get(view.group_index))
                .map(|d| match d {
                    DisplayItem::Group { items, .. } => items.len(),
                    _ => 0,
                })
                .unwrap_or(0),
        }
    }

    pub(crate) fn move_tile_forward(&mut self) {
        let len = self.data.cats.len();
        if len > 0 {
            self.ui.focused_tile = (self.ui.focused_tile + 1) % len;
        }
    }

    pub(crate) fn move_tile_back(&mut self) {
        let len = self.data.cats.len();
        if len > 0 {
            self.ui.focused_tile = (self.ui.focused_tile + len - 1) % len;
        }
    }

    pub(crate) fn move_tile_up(&mut self) {
        let len = self.data.cats.len();
        if len == 0 {
            return;
        }
        let row = self.ui.focused_tile / TILE_COLS;
        let col = self.ui.focused_tile % TILE_COLS;
        if row > 0 {
            let target = (row - 1) * TILE_COLS + col;
            if target < len {
                self.ui.focused_tile = target;
                return;
            }
        }
        self.ui.focused_tile = (self.ui.focused_tile + len - 1) % len;
    }

    pub(crate) fn move_tile_down(&mut self) {
        let len = self.data.cats.len();
        if len == 0 {
            return;
        }
        let row = self.ui.focused_tile / TILE_COLS;
        let col = self.ui.focused_tile % TILE_COLS;
        let target = (row + 1) * TILE_COLS + col;
        if target < len {
            self.ui.focused_tile = target;
        } else {
            self.ui.focused_tile = (self.ui.focused_tile + 1) % len;
        }
    }

    pub(crate) fn move_tile_left(&mut self) {
        let len = self.data.cats.len();
        if len == 0 {
            return;
        }
        let col = self.ui.focused_tile % TILE_COLS;
        if col > 0 {
            self.ui.focused_tile -= 1;
        } else {
            self.ui.focused_tile = (self.ui.focused_tile + len - 1) % len;
        }
    }

    pub(crate) fn move_tile_right(&mut self) {
        let len = self.data.cats.len();
        if len == 0 {
            return;
        }
        let col = self.ui.focused_tile % TILE_COLS;
        if col + 1 < TILE_COLS && self.ui.focused_tile + 1 < len {
            self.ui.focused_tile += 1;
        } else {
            self.ui.focused_tile = (self.ui.focused_tile + 1) % len;
        }
    }

    pub(crate) fn move_up(&mut self) {
        match &mut self.ui.screen {
            Screen::Home => {}
            Screen::Category(view) => {
                let sel = view.list_state.selected().unwrap_or(0);
                if sel > 0 {
                    view.list_state.select(Some(sel - 1));
                }
            }
            Screen::Detail { view, .. } => {
                let sel = view.list_state.selected().unwrap_or(0);
                if sel > 0 {
                    view.list_state.select(Some(sel - 1));
                }
            }
        }
    }

    pub(crate) fn move_down(&mut self) {
        let len = self.active_list_len();
        match &mut self.ui.screen {
            Screen::Home => {}
            Screen::Category(view) => {
                let sel = view.list_state.selected().unwrap_or(0);
                if len > 0 && sel < len - 1 {
                    view.list_state.select(Some(sel + 1));
                }
            }
            Screen::Detail { view, .. } => {
                let sel = view.list_state.selected().unwrap_or(0);
                if len > 0 && sel < len - 1 {
                    view.list_state.select(Some(sel + 1));
                }
            }
        }
    }
}
