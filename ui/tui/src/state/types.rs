use anyhow::Result;
use ratatui::widgets::ListState;
use workflows::status::{StatusItem, StatusReport};

use crate::display::{CatData, Category, DisplayItem};

#[derive(Debug, Default)]
pub(crate) enum RefreshState {
    #[default]
    Idle,
    InProgress,
    Failed(String),
}

#[derive(Debug, Clone)]
pub(crate) struct CategoryView {
    pub(crate) cat: Category,
    pub(crate) list_state: ListState,
    pub(crate) prev_focused_tile: usize,
}

#[derive(Debug)]
pub(crate) struct DetailView {
    pub(crate) cat: Category,
    pub(crate) group_index: usize,
    pub(crate) list_state: ListState,
}

// Flat enum, not a Vec<Screen> stack. A ViewStack was tried and removed: it
// allowed any view to be pushed anywhere (no structural constraint on order),
// and Detail carried no return address — Back relied on the stack's shape at
// runtime. The flat enum encodes the valid navigation graph in the type system.
// Detail::parent is the self-contained return address for Back; there is no
// stack to corrupt or misread.
#[derive(Debug)]
pub(crate) enum Screen {
    Home {
        focused_tile: usize,
    },
    Category(CategoryView),
    Detail {
        parent: CategoryView,
        view: DetailView,
    },
}

impl Default for Screen {
    fn default() -> Self {
        Screen::Home { focused_tile: 0 }
    }
}

impl Screen {
    // Returns None for Home (no item selected) and for Category when a Group
    // is selected (the group itself is not a leaf item). Detail drills through
    // the parent group to return the individual item under the cursor.
    pub(crate) fn selected_status_item(&self, cats: &[CatData]) -> Option<StatusItem> {
        match self {
            Screen::Home { .. } => None,
            Screen::Category(view) => {
                let sel = view.list_state.selected().unwrap_or(0);
                let cd = cats.iter().find(|c| c.cat == view.cat)?;
                match cd.items.get(sel)? {
                    DisplayItem::Single(item) => Some(item.clone()),
                    DisplayItem::Group { .. } => None,
                }
            }
            Screen::Detail { view, .. } => {
                let sel = view.list_state.selected().unwrap_or(0);
                let cd = cats.iter().find(|c| c.cat == view.cat)?;
                match cd.items.get(view.group_index)? {
                    DisplayItem::Group { items, .. } => items.get(sel).cloned(),
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
    LaunchCi {
        repo: String,
        run_url: String,
    },
    #[cfg(feature = "private")]
    LaunchSonarrBlocked {
        title: String,
        error: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Quit,
    ToggleHelp,
    CloseHelp,
    Back,
    MoveTileForward,
    MoveTileBack,
    MoveTileUp,
    MoveTileDown,
    MoveTileLeft,
    MoveTileRight,
    MoveUp,
    MoveDown,
    Enter,
    Investigate,
    Refresh,
}

pub(crate) enum Effect {
    Quit,
    OpenUrl(String),
    LaunchCi {
        repo: String,
        run_url: String,
    },
    #[cfg(feature = "private")]
    LaunchSonarrBlocked {
        title: String,
        error: String,
    },
    StartRefresh,
    WriteCache(String),
}

pub(crate) enum Msg {
    Action(Action),
    Tick,
    FetchResult(Result<StatusReport>),
}
