use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state::{Action, App, Screen};

pub(crate) fn key_to_action(app: &App, key: KeyEvent) -> Option<Action> {
    let can_go_back = matches!(app.ui.screen, Screen::Detail { .. });

    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            return Some(Action::Quit);
        }
        (KeyCode::Char('?'), _) => return Some(Action::ToggleHelp),
        (KeyCode::Char('r'), _) => return Some(Action::Refresh),
        (KeyCode::Esc, _) if app.ui.show_help => return Some(Action::CloseHelp),
        (KeyCode::Esc, _) if can_go_back => return Some(Action::Back),
        _ => {}
    }

    if app.ui.show_help {
        return None;
    }

    list_keys(key)
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

#[cfg(test)]
mod tests {
    use super::key_to_action;
    use crate::display::{DisplayItem, Filter, ListSnapshot};
    use crate::state::{Action, App, DetailView, Screen, UiState};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rstest::rstest;

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ch(c: char) -> KeyEvent {
        k(KeyCode::Char(c))
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn detail_app() -> App {
        let snapshot = ListSnapshot {
            items: vec![DisplayItem::Group {
                label: "group".to_string(),
                items: vec![],
            }],
            selected: 0,
            filter: Filter::default(),
        };
        App {
            ui: UiState {
                screen: Screen::Detail {
                    parent: snapshot,
                    view: DetailView {
                        group_index: 0,
                        list_state: ratatui::widgets::ListState::default(),
                    },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    #[rstest]
    #[case(ch('q'), Some(Action::Quit))]
    #[case(ctrl('c'), Some(Action::Quit))]
    #[case(ch('?'), Some(Action::ToggleHelp))]
    #[case(ch('r'), Some(Action::Refresh))]
    fn universal_keys_fire_in_unified_list(
        #[case] key: KeyEvent,
        #[case] expected: Option<Action>,
    ) {
        assert_eq!(key_to_action(&App::default(), key), expected);
    }

    #[rstest]
    #[case(ch('q'), Some(Action::Quit))]
    #[case(ctrl('c'), Some(Action::Quit))]
    #[case(ch('?'), Some(Action::ToggleHelp))]
    #[case(ch('r'), Some(Action::Refresh))]
    fn universal_keys_fire_in_detail(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&detail_app(), key), expected);
    }

    #[test]
    fn esc_closes_help_when_help_is_open() {
        let app = App {
            ui: UiState {
                show_help: true,
                ..UiState::default()
            },
            ..App::default()
        };
        assert_eq!(
            key_to_action(&app, k(KeyCode::Esc)),
            Some(Action::CloseHelp)
        );
    }

    #[test]
    fn esc_goes_back_from_detail() {
        assert_eq!(
            key_to_action(&detail_app(), k(KeyCode::Esc)),
            Some(Action::Back)
        );
    }

    #[test]
    fn esc_does_nothing_from_unified_list_without_help() {
        assert_eq!(key_to_action(&App::default(), k(KeyCode::Esc)), None);
    }

    #[test]
    fn keys_blocked_when_help_is_open() {
        let app = App {
            ui: UiState {
                show_help: true,
                ..UiState::default()
            },
            ..App::default()
        };
        assert_eq!(key_to_action(&app, k(KeyCode::Enter)), None);
        assert_eq!(key_to_action(&app, ch('j')), None);
    }

    #[rstest]
    #[case(k(KeyCode::Up), Some(Action::MoveUp))]
    #[case(ch('k'), Some(Action::MoveUp))]
    #[case(ch('h'), Some(Action::MoveUp))]
    #[case(k(KeyCode::Down), Some(Action::MoveDown))]
    #[case(ch('j'), Some(Action::MoveDown))]
    #[case(ch('l'), Some(Action::MoveDown))]
    #[case(k(KeyCode::Enter), Some(Action::Enter))]
    #[case(ch('i'), Some(Action::Investigate))]
    #[case(ch('x'), None)]
    fn unified_list_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&App::default(), key), expected);
    }

    #[rstest]
    #[case(k(KeyCode::Up), Some(Action::MoveUp))]
    #[case(ch('k'), Some(Action::MoveUp))]
    #[case(ch('h'), Some(Action::MoveUp))]
    #[case(k(KeyCode::Down), Some(Action::MoveDown))]
    #[case(ch('j'), Some(Action::MoveDown))]
    #[case(ch('l'), Some(Action::MoveDown))]
    #[case(k(KeyCode::Enter), Some(Action::Enter))]
    #[case(ch('i'), Some(Action::Investigate))]
    #[case(ch('x'), None)]
    fn detail_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&detail_app(), key), expected);
    }
}
