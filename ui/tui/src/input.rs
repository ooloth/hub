use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state::{Action, App, Screen};

pub(crate) fn key_to_action(app: &App, key: KeyEvent) -> Option<Action> {
    let can_go_back = !matches!(app.ui.screen, Screen::Home);

    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            return Some(Action::Quit);
        }
        (KeyCode::Char('?'), _) => return Some(Action::ToggleHelp),
        (KeyCode::Esc, _) if app.ui.show_help => return Some(Action::CloseHelp),
        (KeyCode::Esc, _) if can_go_back => return Some(Action::Back),
        _ => {}
    }

    if app.ui.show_help {
        return None;
    }

    match app.current_screen() {
        Screen::Home => home_keys(key),
        Screen::Category(_) | Screen::Detail { .. } => list_keys(key),
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

#[cfg(test)]
mod tests {
    use super::key_to_action;
    use crate::display::Category;
    use crate::state::{Action, App, CategoryView, Screen, UiState};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::widgets::ListState;
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

    fn category_app() -> App {
        let mut ls = ListState::default();
        ls.select(Some(0));
        App {
            ui: UiState {
                screen: Screen::Category(CategoryView {
                    cat: Category::Errors,
                    list_state: ls,
                }),
                ..UiState::default()
            },
            ..App::default()
        }
    }

    #[rstest]
    #[case(ch('q'), Some(Action::Quit))]
    #[case(ctrl('c'), Some(Action::Quit))]
    #[case(ch('?'), Some(Action::ToggleHelp))]
    fn universal_keys_fire_in_home_view(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&App::default(), key), expected);
    }

    #[rstest]
    #[case(ch('q'), Some(Action::Quit))]
    #[case(ctrl('c'), Some(Action::Quit))]
    #[case(ch('?'), Some(Action::ToggleHelp))]
    fn universal_keys_fire_in_category_view(
        #[case] key: KeyEvent,
        #[case] expected: Option<Action>,
    ) {
        assert_eq!(key_to_action(&category_app(), key), expected);
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
    fn esc_goes_back_when_can_go_back() {
        assert_eq!(
            key_to_action(&category_app(), k(KeyCode::Esc)),
            Some(Action::Back)
        );
    }

    #[test]
    fn esc_does_nothing_from_home_without_help() {
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
    #[case(k(KeyCode::Tab), Some(Action::MoveTileForward))]
    #[case(k(KeyCode::BackTab), Some(Action::MoveTileBack))]
    #[case(k(KeyCode::Right), Some(Action::MoveTileRight))]
    #[case(ch('l'), Some(Action::MoveTileRight))]
    #[case(k(KeyCode::Left), Some(Action::MoveTileLeft))]
    #[case(ch('h'), Some(Action::MoveTileLeft))]
    #[case(k(KeyCode::Down), Some(Action::MoveTileDown))]
    #[case(ch('j'), Some(Action::MoveTileDown))]
    #[case(k(KeyCode::Up), Some(Action::MoveTileUp))]
    #[case(ch('k'), Some(Action::MoveTileUp))]
    #[case(k(KeyCode::Enter), Some(Action::Enter))]
    #[case(ch('i'), Some(Action::Investigate))]
    #[case(ch('x'), None)]
    fn home_view_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
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
    fn category_view_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&category_app(), key), expected);
    }

    #[test]
    fn h_and_l_mean_different_things_in_home_vs_list_views() {
        let home = App::default();
        let cat = category_app();
        assert_eq!(key_to_action(&home, ch('h')), Some(Action::MoveTileLeft));
        assert_eq!(key_to_action(&cat, ch('h')), Some(Action::MoveUp));
        assert_eq!(key_to_action(&home, ch('l')), Some(Action::MoveTileRight));
        assert_eq!(key_to_action(&cat, ch('l')), Some(Action::MoveDown));
    }
}
