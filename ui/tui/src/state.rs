use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use ratatui::widgets::ListState;
use workflows::status::StatusReport;

use crate::display::{
    build_cats, item_investigation, item_url, CatData, Category, DisplayItem, InvestigationKind,
};

pub(crate) const TILE_COLS: usize = 2;

#[derive(Debug, Default)]
pub(crate) struct UiState {
    pub(crate) focused_tile: usize,
    pub(crate) screen: Screen,
    pub(crate) show_help: bool,
    pub(crate) flash: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) enum RefreshState {
    #[default]
    Idle,
    InProgress,
    Failed(String),
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

#[derive(Debug, Clone)]
pub(crate) struct CategoryView {
    pub(crate) cat: Category,
    pub(crate) list_state: ListState,
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
#[derive(Debug, Default)]
pub(crate) enum Screen {
    #[default]
    Home,
    Category(CategoryView),
    Detail {
        parent: CategoryView,
        view: DetailView,
    },
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

    pub(crate) fn update(&mut self, action: Action) -> Vec<Effect> {
        self.ui.flash = None;
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
            Action::Back => {
                self.ui.screen = match std::mem::take(&mut self.ui.screen) {
                    Screen::Detail { parent, .. } => Screen::Category(parent),
                    _ => Screen::Home,
                };
                vec![]
            }
            Action::MoveTileForward => {
                self.move_tile_forward();
                vec![]
            }
            Action::MoveTileBack => {
                self.move_tile_back();
                vec![]
            }
            Action::MoveTileUp => {
                self.move_tile_up();
                vec![]
            }
            Action::MoveTileDown => {
                self.move_tile_down();
                vec![]
            }
            Action::MoveTileLeft => {
                self.move_tile_left();
                vec![]
            }
            Action::MoveTileRight => {
                self.move_tile_right();
                vec![]
            }
            Action::MoveUp => {
                self.move_up();
                vec![]
            }
            Action::MoveDown => {
                self.move_down();
                vec![]
            }
            Action::Enter => match compute_enter_action(self) {
                EnterAction::None => vec![],
                EnterAction::OpenUrl(url) => vec![Effect::OpenUrl(url)],
                EnterAction::OpenCategory { cat } => {
                    let len = self
                        .data
                        .cats
                        .iter()
                        .find(|c| c.cat == cat)
                        .map(|c| c.items.len())
                        .unwrap_or(0);
                    let mut ls = ListState::default();
                    if len > 0 {
                        ls.select(Some(0));
                    }
                    self.ui.screen = Screen::Category(CategoryView {
                        cat,
                        list_state: ls,
                    });
                    vec![]
                }
                EnterAction::OpenDetail {
                    cat,
                    group_index,
                    item_count,
                } => {
                    let parent = if let Screen::Category(cv) = &self.ui.screen {
                        cv.clone()
                    } else {
                        return vec![];
                    };
                    let mut ds = ListState::default();
                    if item_count > 0 {
                        ds.select(Some(0));
                    }
                    self.ui.screen = Screen::Detail {
                        parent,
                        view: DetailView {
                            cat,
                            group_index,
                            list_state: ds,
                        },
                    };
                    vec![]
                }
            },
            Action::Investigate => match compute_investigate_action(self) {
                InvestigateAction::LaunchCi { repo, run_url } => {
                    vec![Effect::LaunchCi { repo, run_url }]
                }
                InvestigateAction::None => {
                    self.ui.flash = Some("No investigation mapped".to_string());
                    vec![]
                }
            },
        }
    }

    pub(crate) fn selected_url(&self) -> Option<&str> {
        match &self.ui.screen {
            Screen::Home => None,
            Screen::Category(view) => {
                let sel = view.list_state.selected().unwrap_or(0);
                let cd = self.data.cat_data(view.cat)?;
                match cd.items.get(sel)? {
                    DisplayItem::Single(item) => item_url(item),
                    DisplayItem::Group { .. } => None,
                }
            }
            Screen::Detail { view, .. } => {
                let sel = view.list_state.selected().unwrap_or(0);
                let cd = self.data.cat_data(view.cat)?;
                match cd.items.get(view.group_index)? {
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
}

pub(crate) enum Effect {
    Quit,
    OpenUrl(String),
    LaunchCi { repo: String, run_url: String },
    StartRefresh,
    WriteCache(String),
}

pub(crate) enum Msg {
    Action(Action),
    Tick,
    FetchResult(Result<StatusReport>),
}

pub(crate) fn compute_enter_action(app: &App) -> EnterAction {
    match app.current_screen() {
        Screen::Home => {
            if app.data.cats.is_empty() {
                return EnterAction::None;
            }
            EnterAction::OpenCategory {
                cat: app.data.cats[app.ui.focused_tile].cat,
            }
        }
        Screen::Category(view) => {
            let sel = view.list_state.selected().unwrap_or(0);
            let Some(cd) = app.data.cat_data(view.cat) else {
                return EnterAction::None;
            };
            match cd.items.get(sel) {
                Some(DisplayItem::Group { items, .. }) => EnterAction::OpenDetail {
                    cat: view.cat,
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
        Screen::Detail { .. } => app
            .selected_url()
            .map(|u| EnterAction::OpenUrl(u.to_string()))
            .unwrap_or(EnterAction::None),
    }
}

pub(crate) fn compute_investigate_action(app: &App) -> InvestigateAction {
    let item = match app.current_screen() {
        Screen::Home => return InvestigateAction::None,
        Screen::Category(view) => {
            let sel = view.list_state.selected().unwrap_or(0);
            let Some(cd) = app.data.cat_data(view.cat) else {
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
        Screen::Detail { view, .. } => {
            let sel = view.list_state.selected().unwrap_or(0);
            let Some(cd) = app.data.cat_data(view.cat) else {
                return InvestigateAction::None;
            };
            let Some(display_item) = cd.items.get(view.group_index) else {
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
    match item_investigation(item) {
        Some(InvestigationKind::Ci { repo, run_url }) => {
            InvestigateAction::LaunchCi { repo, run_url }
        }
        None => InvestigateAction::None,
    }
}

pub(crate) fn handle_msg(app: &mut App, msg: Msg) -> Result<Vec<Effect>> {
    match msg {
        Msg::Action(action) => Ok(app.update(action)),
        Msg::Tick => {
            if !matches!(app.data.refresh_state, RefreshState::InProgress) {
                app.data.refresh_state = RefreshState::InProgress;
                Ok(vec![Effect::StartRefresh])
            } else {
                Ok(vec![])
            }
        }
        Msg::FetchResult(Ok(report)) => {
            let json =
                serde_json::to_string(&report).context("failed to serialize status report")?;
            let cats = build_cats(report.items);
            app.ui.focused_tile = app.ui.focused_tile.min(cats.len().saturating_sub(1));
            app.data.cats = cats;
            app.ui.screen = Screen::Home;
            app.data.last_updated = Some(Utc::now());
            app.data.refresh_state = RefreshState::Idle;
            Ok(vec![Effect::WriteCache(json)])
        }
        Msg::FetchResult(Err(e)) => {
            app.data.refresh_state = RefreshState::Failed(e.to_string());
            Ok(vec![])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compute_investigate_action, handle_msg, Action, App, Category, DataState, Effect,
        InvestigateAction, Msg, RefreshState, Screen, UiState,
    };
    use crate::display::{CatData, DisplayItem};
    use workflows::status::{StatusItem, StatusReport};

    #[test]
    fn investigate_action_launches_ci_from_category_selection() {
        let mut app = app_with_cat(Category::Errors, vec![DisplayItem::Single(ci_failure())]);
        apply(&mut app, &[Action::Enter]);

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
        let mut app = app_with_cat(
            Category::Errors,
            vec![DisplayItem::Group {
                label: "group".to_string(),
                items: vec![ci_failure()],
            }],
        );
        apply(&mut app, &[Action::Enter, Action::Enter]);

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
        let mut app = app_with_cat(
            Category::Issues,
            vec![DisplayItem::Single(StatusItem::Issue(domain::Issue {
                number: 31,
                title: "TUI investigation".to_string(),
                repo: domain::RepoSlug::new("ooloth", "hub"),
                url: "https://github.com/ooloth/hub/issues/31".to_string(),
                age: chrono::Duration::zero(),
                urgency: domain::Urgency::Low,
                labels: vec![],
            }))],
        );
        apply(&mut app, &[Action::Enter]);

        assert_eq!(compute_investigate_action(&app), InvestigateAction::None);
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

    #[test]
    fn update_enter_on_home_opens_category() {
        let mut app = app_with_cat(Category::Errors, vec![DisplayItem::Single(ci_failure())]);
        app.update(Action::Enter);
        assert!(matches!(app.current_screen(), Screen::Category(_)));
    }

    #[test]
    fn enter_on_group_in_category_opens_detail() {
        let mut app = app_with_cat(
            Category::Errors,
            vec![DisplayItem::Group {
                label: "hub".to_string(),
                items: vec![ci_failure()],
            }],
        );
        apply(&mut app, &[Action::Enter, Action::Enter]);
        assert!(matches!(app.current_screen(), Screen::Detail { .. }));
    }

    #[test]
    fn back_from_detail_returns_to_category() {
        let mut app = app_with_cat(
            Category::Errors,
            vec![DisplayItem::Group {
                label: "hub".to_string(),
                items: vec![ci_failure()],
            }],
        );
        apply(&mut app, &[Action::Enter, Action::Enter, Action::Back]);
        assert!(matches!(app.current_screen(), Screen::Category(_)));
    }

    #[test]
    fn back_from_category_returns_to_home() {
        let mut app = app_with_cat(Category::Errors, vec![DisplayItem::Single(ci_failure())]);
        apply(&mut app, &[Action::Enter, Action::Back]);
        assert!(matches!(app.current_screen(), Screen::Home));
    }

    #[test]
    fn back_does_nothing_from_home() {
        let mut app = App::default();
        app.update(Action::Back);
        assert!(matches!(app.current_screen(), Screen::Home));
    }

    #[test]
    fn full_navigation_round_trip() {
        let mut app = app_with_cat(
            Category::Errors,
            vec![DisplayItem::Group {
                label: "hub".to_string(),
                items: vec![ci_failure()],
            }],
        );
        apply(&mut app, &[Action::Enter]);
        assert!(matches!(app.current_screen(), Screen::Category(_)));
        apply(&mut app, &[Action::Enter]);
        assert!(matches!(app.current_screen(), Screen::Detail { .. }));
        apply(&mut app, &[Action::Back]);
        assert!(matches!(app.current_screen(), Screen::Category(_)));
        apply(&mut app, &[Action::Back]);
        assert!(matches!(app.current_screen(), Screen::Home));
    }

    #[test]
    fn handle_msg_tick_when_idle_starts_refresh() {
        let mut app = App::default();
        let effects = handle_msg(&mut app, Msg::Tick).unwrap();
        assert!(matches!(app.data.refresh_state, RefreshState::InProgress));
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
    fn handle_msg_fetch_ok_sets_idle_and_updates_cats() {
        let mut app = App {
            data: DataState {
                refresh_state: RefreshState::InProgress,
                ..DataState::default()
            },
            ..App::default()
        };
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        assert!(matches!(app.data.refresh_state, RefreshState::Idle));
        assert!(!app.data.cats.is_empty());
        assert!(app.data.last_updated.is_some());
    }

    #[test]
    fn handle_msg_fetch_ok_resets_to_home() {
        let mut app = app_with_cat(Category::Errors, vec![DisplayItem::Single(ci_failure())]);
        apply(&mut app, &[Action::Enter]);
        app.data.refresh_state = RefreshState::InProgress;
        handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
        assert!(matches!(app.current_screen(), Screen::Home));
    }

    #[test]
    fn handle_msg_fetch_ok_returns_write_cache_effect() {
        let mut app = App::default();
        let effects = handle_msg(&mut app, Msg::FetchResult(Ok(report_with_ci()))).unwrap();
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

    fn app_with_cat(cat: Category, items: Vec<DisplayItem>) -> App {
        App {
            data: DataState {
                cats: vec![CatData { cat, items }],
                ..DataState::default()
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
        }
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
