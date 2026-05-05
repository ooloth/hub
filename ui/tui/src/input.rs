use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::state::{
    compute_enter_action, compute_investigate_action, App, EnterAction, InvestigateAction, View,
};

pub(crate) enum Effect {
    None,
    Quit,
    OpenUrl(String),
    LaunchCi { repo: String, run_url: String },
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) -> Effect {
    app.flash = None;
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            return Effect::Quit;
        }

        (KeyCode::Char('?'), _) => app.show_help = !app.show_help,
        (KeyCode::Esc, _) if app.show_help => app.show_help = false,

        // Esc = back one level
        (KeyCode::Esc, _) if app.views.len() > 1 => app.pop_view(),

        _ if app.show_help => {}

        // Home tile navigation
        (KeyCode::Tab, _) if matches!(app.current_view(), View::Home) => app.move_tile_forward(),
        (KeyCode::BackTab, _) if matches!(app.current_view(), View::Home) => app.move_tile_back(),
        (KeyCode::Right, _) | (KeyCode::Char('l'), _)
            if matches!(app.current_view(), View::Home) =>
        {
            app.move_tile_right()
        }
        (KeyCode::Left, _) | (KeyCode::Char('h'), _)
            if matches!(app.current_view(), View::Home) =>
        {
            app.move_tile_left()
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _)
            if matches!(app.current_view(), View::Home) =>
        {
            app.move_tile_down()
        }
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) if matches!(app.current_view(), View::Home) => {
            app.move_tile_up()
        }

        // List navigation
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) | (KeyCode::Char('h'), _) => app.move_up(),
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) | (KeyCode::Char('l'), _) => app.move_down(),

        // Enter = drill in or open URL
        (KeyCode::Enter, _) => match compute_enter_action(app) {
            EnterAction::None => {}
            EnterAction::OpenUrl(url) => return Effect::OpenUrl(url),
            EnterAction::OpenCategory { cat } => {
                let len = app
                    .cats
                    .iter()
                    .find(|c| c.cat == cat)
                    .map(|c| c.items.len())
                    .unwrap_or(0);
                let mut ls = ListState::default();
                if len > 0 {
                    ls.select(Some(0));
                }
                app.push_view(View::Category {
                    cat,
                    list_state: ls,
                });
            }
            EnterAction::OpenDetail {
                cat,
                group_index,
                item_count,
            } => {
                let mut ds = ListState::default();
                if item_count > 0 {
                    ds.select(Some(0));
                }
                app.push_view(View::Detail {
                    cat,
                    group_index,
                    list_state: ds,
                });
            }
        },

        // i = investigate CI failure
        (KeyCode::Char('i'), _) => match compute_investigate_action(app) {
            InvestigateAction::LaunchCi { repo, run_url } => {
                return Effect::LaunchCi { repo, run_url };
            }
            InvestigateAction::None => {
                app.flash = Some("No investigation mapped".to_string());
            }
        },

        _ => {}
    }
    Effect::None
}
