use anyhow::Result;
use ratatui::widgets::ListState;
use workflows::status::{StatusItem, StatusReport};

use crate::display::{Category, DisplayItem, Filter, ListSnapshot};

#[derive(Debug, Default)]
pub(crate) enum RefreshState {
    #[default]
    Idle,
    InProgress,
    /// Partial refresh: some sources succeeded, others failed.
    Partial(Vec<String>),
    Failed(String),
}

#[derive(Debug)]
pub(crate) struct DetailView {
    pub(crate) group_index: usize,
    pub(crate) list_state: ListState,
}

// Flat enum encoding the valid navigation graph in the type system.
// UnifiedList is the default (top-level) screen. Detail carries a
// ListSnapshot as its return address — pressing Back restores the
// list to the exact items/selection/filter state before drill-in.
#[derive(Debug)]
pub(crate) enum Screen {
    UnifiedList {
        items: Vec<DisplayItem>,
        selected: usize,
        filter: Filter,
    },
    Detail {
        parent: ListSnapshot,
        view: DetailView,
    },
}

impl Default for Screen {
    fn default() -> Self {
        Screen::UnifiedList {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
        }
    }
}

impl Screen {
    pub(crate) fn selected_status_item(&self) -> Option<StatusItem> {
        match self {
            Screen::UnifiedList {
                items, selected, ..
            } => match items.get(*selected)? {
                DisplayItem::Single(item) => Some(item.clone()),
                DisplayItem::Group { .. } => None,
            },
            Screen::Detail { parent, view } => {
                let sel = view.list_state.selected().unwrap_or(0);
                match parent.items.get(view.group_index)? {
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
    OpenDetail {
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
    LaunchLoki {
        project: String,
        env: String,
        title: String,
        message: String,
        line: String,
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
    MoveUp,
    MoveDown,
    Enter,
    Investigate,
    Refresh,
    // Filter actions — only take effect from UnifiedList in normal mode.
    FilterCategory(Category),
    ClearFilter,
    StartQuery,
    AppendQuery(char),
    BackspaceQuery,
    CommitQuery,
    CancelQuery,
}

pub(crate) enum Effect {
    Quit,
    OpenUrl(String),
    LaunchCi {
        repo: String,
        run_url: String,
    },
    LaunchLoki {
        project: String,
        env: String,
        title: String,
        message: String,
        line: String,
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
