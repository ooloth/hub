use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state::{Action, App, View};

pub(crate) fn key_to_action(app: &App, key: KeyEvent) -> Option<Action> {
    let can_go_back = app.views.can_go_back();

    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            return Some(Action::Quit);
        }
        (KeyCode::Char('?'), _) => return Some(Action::ToggleHelp),
        (KeyCode::Esc, _) if app.show_help => return Some(Action::CloseHelp),
        (KeyCode::Esc, _) if can_go_back => return Some(Action::Back),
        _ => {}
    }

    if app.show_help {
        return None;
    }

    match app.current_view() {
        View::Home => home_keys(key),
        View::Category { .. } | View::Detail { .. } => list_keys(key),
    }
}

fn home_keys(key: KeyEvent) -> Option<Action> {
    match (key.code, key.modifiers) {
        (KeyCode::Tab, _) => Some(Action::MoveTileForward),
        (KeyCode::BackTab, _) => Some(Action::MoveTileBack),
        (KeyCode::Right, _) | (KeyCode::Char('l'), _) => Some(Action::MoveTileRight),
        (KeyCode::Left, _) | (KeyCode::Char('h'), _) => Some(Action::MoveTileLeft),
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => Some(Action::MoveTileDown),
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => Some(Action::MoveTileUp),
        (KeyCode::Enter, _) => Some(Action::Enter),
        (KeyCode::Char('i'), _) => Some(Action::Investigate),
        _ => None,
    }
}

fn list_keys(key: KeyEvent) -> Option<Action> {
    match (key.code, key.modifiers) {
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
