use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state::{Action, App, View};

pub(crate) fn key_to_action(app: &App, key: KeyEvent) -> Option<Action> {
    let on_home = matches!(app.current_view(), View::Home);
    let can_go_back = app.views.len() > 1;

    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(Action::Quit),

        (KeyCode::Char('?'), _) => Some(Action::ToggleHelp),
        (KeyCode::Esc, _) if app.show_help => Some(Action::CloseHelp),
        (KeyCode::Esc, _) if can_go_back => Some(Action::Back),

        _ if app.show_help => None,

        // Home tile navigation
        (KeyCode::Tab, _) if on_home => Some(Action::MoveTileForward),
        (KeyCode::BackTab, _) if on_home => Some(Action::MoveTileBack),
        (KeyCode::Right, _) | (KeyCode::Char('l'), _) if on_home => Some(Action::MoveTileRight),
        (KeyCode::Left, _) | (KeyCode::Char('h'), _) if on_home => Some(Action::MoveTileLeft),
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) if on_home => Some(Action::MoveTileDown),
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) if on_home => Some(Action::MoveTileUp),

        // List navigation
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) | (KeyCode::Char('h'), _) => {
            Some(Action::MoveUp)
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) | (KeyCode::Char('l'), _) => {
            Some(Action::MoveDown)
        }

        (KeyCode::Enter, _) => Some(Action::Enter),
        (KeyCode::Char('i'), _) => Some(Action::Investigate),

        _ => None,
    }
}
