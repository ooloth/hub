pub(crate) mod app;
pub(crate) mod types;
pub(crate) mod update;

pub(crate) use app::{App, DataState, UiState};
pub(crate) use types::{
    Action, DetailMode, Effect, InvestigateAction, Msg, PrOwnership, PrPrevScreen, RefreshState,
    ReviewSkill, Screen, SubmenuState,
};
pub(crate) use update::{compute_investigate_action, handle_msg};
