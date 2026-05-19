use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::display::Category;
use crate::state::{Action, App, Screen};

pub(crate) fn key_to_action(app: &App, key: KeyEvent) -> Option<Action> {
    // Query mode intercepts all keys (Ctrl-C still quits).
    if app.ui.query_input.is_some() {
        return query_mode_key(key);
    }

    let can_go_back = matches!(
        app.ui.screen,
        Screen::Detail { .. } | Screen::IssueDetail { .. }
    );
    let has_filter = match &app.ui.screen {
        Screen::UnifiedList { filter, .. } => !filter.is_empty(),
        _ => false,
    };

    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            return Some(Action::Quit);
        }
        (KeyCode::Char('?'), _) => return Some(Action::ToggleHelp),
        (KeyCode::Char('r'), _) => return Some(Action::Refresh),
        (KeyCode::Esc, _) if app.ui.show_help => return Some(Action::CloseHelp),
        (KeyCode::Esc, _) if can_go_back => return Some(Action::Back),
        (KeyCode::Esc, _) if has_filter => return Some(Action::ClearFilter),
        _ => {}
    }

    if app.ui.show_help {
        return None;
    }

    // gg: second g completes the sequence.
    if app.ui.pending_g && key.code == KeyCode::Char('g') {
        return Some(Action::MoveToTop);
    }

    match app.current_screen() {
        Screen::UnifiedList { .. } => unified_list_keys(key),
        Screen::Detail { .. } => list_keys(key),
        Screen::IssueDetail { .. } => issue_reader_keys(key),
    }
}

fn query_mode_key(key: KeyEvent) -> Option<Action> {
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('c'), KeyModifiers::CONTROL)
    ) {
        return Some(Action::Quit);
    }
    match key.code {
        KeyCode::Esc => Some(Action::CancelQuery),
        KeyCode::Enter => Some(Action::CommitQuery),
        KeyCode::Backspace => Some(Action::BackspaceQuery),
        KeyCode::Char(c) => Some(Action::AppendQuery(c)),
        _ => None,
    }
}

fn unified_list_keys(key: KeyEvent) -> Option<Action> {
    match (key.code, key.modifiers) {
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) | (KeyCode::Char('h'), _) => {
            Some(Action::MoveUp)
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) | (KeyCode::Char('l'), _) => {
            Some(Action::MoveDown)
        }
        (KeyCode::Char('g'), _) => Some(Action::PendingG),
        (KeyCode::Char('G'), _) => Some(Action::MoveToBottom),
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => Some(Action::MovePageUp),
        (KeyCode::Char('d'), KeyModifiers::CONTROL) => Some(Action::MovePageDown),
        (KeyCode::Enter, _) => Some(Action::Enter),
        (KeyCode::Char('i'), _) => Some(Action::Investigate),
        (KeyCode::Char('p'), _) => Some(Action::FilterCategory(Category::Prs)),
        (KeyCode::Char('e'), _) => Some(Action::FilterCategory(Category::Errors)),
        (KeyCode::Char('o'), _) => Some(Action::FilterCategory(Category::Issues)),
        (KeyCode::Char('a'), _) => Some(Action::ClearFilter),
        (KeyCode::Char('/'), _) => Some(Action::StartQuery),
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
        (KeyCode::Char('g'), _) => Some(Action::PendingG),
        (KeyCode::Char('G'), _) => Some(Action::MoveToBottom),
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => Some(Action::MovePageUp),
        (KeyCode::Char('d'), KeyModifiers::CONTROL) => Some(Action::MovePageDown),
        (KeyCode::Enter, _) => Some(Action::Enter),
        (KeyCode::Char('i'), _) => Some(Action::Investigate),
        _ => None,
    }
}

fn issue_reader_keys(key: KeyEvent) -> Option<Action> {
    match (key.code, key.modifiers) {
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => Some(Action::MoveUp),
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => Some(Action::MoveDown),
        (KeyCode::Char('g'), _) => Some(Action::PendingG),
        (KeyCode::Char('G'), _) => Some(Action::MoveToBottom),
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => Some(Action::MovePageUp),
        (KeyCode::Char('d'), KeyModifiers::CONTROL) => Some(Action::MovePageDown),
        (KeyCode::Enter, _) => Some(Action::Enter),
        (KeyCode::Char('a'), _) => Some(Action::ApproveForAgent),
        (KeyCode::Char('i'), _) => Some(Action::Investigate),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::key_to_action;
    use crate::display::{Category, DisplayItem, Filter, ListSnapshot};
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

    fn filtered_app(category: Category) -> App {
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items: vec![],
                    selected: 0,
                    filter: Filter {
                        category: Some(category),
                        query: None,
                    },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn querying_app() -> App {
        App {
            ui: UiState {
                query_input: Some("hub".to_string()),
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn issue_detail_app() -> App {
        let parent = ListSnapshot {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
        };
        App {
            ui: UiState {
                screen: Screen::IssueDetail {
                    parent,
                    issue: domain::Issue {
                        number: 1,
                        title: "test".to_string(),
                        repo: domain::RepoSlug::new("ooloth", "hub"),
                        url: "https://github.com/ooloth/hub/issues/1".to_string(),
                        age: chrono::Duration::zero(),
                        urgency: domain::Urgency::Low,
                        labels: vec![],
                        body: None,
                    },
                    scroll: 0,
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
    fn esc_clears_filter_when_filter_active() {
        assert_eq!(
            key_to_action(&filtered_app(Category::Prs), k(KeyCode::Esc)),
            Some(Action::ClearFilter)
        );
    }

    #[test]
    fn esc_does_nothing_from_unified_list_without_filter() {
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
    #[case(ch('g'), Some(Action::PendingG))]
    #[case(ch('G'), Some(Action::MoveToBottom))]
    #[case(ctrl('u'), Some(Action::MovePageUp))]
    #[case(ctrl('d'), Some(Action::MovePageDown))]
    #[case(k(KeyCode::Enter), Some(Action::Enter))]
    #[case(ch('i'), Some(Action::Investigate))]
    #[case(ch('p'), Some(Action::FilterCategory(Category::Prs)))]
    #[case(ch('e'), Some(Action::FilterCategory(Category::Errors)))]
    #[case(ch('o'), Some(Action::FilterCategory(Category::Issues)))]
    #[case(ch('a'), Some(Action::ClearFilter))]
    #[case(ch('/'), Some(Action::StartQuery))]
    #[case(ch('x'), None)]
    fn unified_list_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&App::default(), key), expected);
    }

    #[test]
    fn gg_fires_move_to_top_when_pending_g_is_armed() {
        let app = App {
            ui: UiState {
                pending_g: true,
                ..UiState::default()
            },
            ..App::default()
        };
        assert_eq!(key_to_action(&app, ch('g')), Some(Action::MoveToTop));
    }

    #[test]
    fn first_g_arms_pending_g_in_unified_list() {
        assert_eq!(
            key_to_action(&App::default(), ch('g')),
            Some(Action::PendingG)
        );
    }

    #[rstest]
    #[case(k(KeyCode::Up), Some(Action::MoveUp))]
    #[case(ch('k'), Some(Action::MoveUp))]
    #[case(ch('h'), Some(Action::MoveUp))]
    #[case(k(KeyCode::Down), Some(Action::MoveDown))]
    #[case(ch('j'), Some(Action::MoveDown))]
    #[case(ch('l'), Some(Action::MoveDown))]
    #[case(ch('g'), Some(Action::PendingG))]
    #[case(ch('G'), Some(Action::MoveToBottom))]
    #[case(ctrl('u'), Some(Action::MovePageUp))]
    #[case(ctrl('d'), Some(Action::MovePageDown))]
    #[case(k(KeyCode::Enter), Some(Action::Enter))]
    #[case(ch('i'), Some(Action::Investigate))]
    #[case(ch('p'), None)]
    #[case(ch('e'), None)]
    #[case(ch('o'), None)]
    #[case(ch('a'), None)]
    #[case(ch('/'), None)]
    #[case(ch('x'), None)]
    fn detail_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&detail_app(), key), expected);
    }

    #[rstest]
    #[case(ctrl('c'), Some(Action::Quit))]
    #[case(k(KeyCode::Esc), Some(Action::CancelQuery))]
    #[case(k(KeyCode::Enter), Some(Action::CommitQuery))]
    #[case(k(KeyCode::Backspace), Some(Action::BackspaceQuery))]
    #[case(ch('j'), Some(Action::AppendQuery('j')))]
    #[case(ch('q'), Some(Action::AppendQuery('q')))]
    #[case(ch('?'), Some(Action::AppendQuery('?')))]
    fn query_mode_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&querying_app(), key), expected);
    }

    #[rstest]
    #[case(k(KeyCode::Up), Some(Action::MoveUp))]
    #[case(ch('k'), Some(Action::MoveUp))]
    #[case(k(KeyCode::Down), Some(Action::MoveDown))]
    #[case(ch('j'), Some(Action::MoveDown))]
    #[case(ch('g'), Some(Action::PendingG))]
    #[case(ch('G'), Some(Action::MoveToBottom))]
    #[case(ctrl('u'), Some(Action::MovePageUp))]
    #[case(ctrl('d'), Some(Action::MovePageDown))]
    #[case(k(KeyCode::Enter), Some(Action::Enter))]
    #[case(ch('a'), Some(Action::ApproveForAgent))]
    #[case(ch('i'), Some(Action::Investigate))]
    #[case(ch('o'), None)]
    #[case(ch('p'), None)]
    #[case(ch('e'), None)]
    #[case(ch('/'), None)]
    #[case(ch('x'), None)]
    fn issue_detail_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&issue_detail_app(), key), expected);
    }

    #[test]
    fn esc_goes_back_from_issue_detail() {
        assert_eq!(
            key_to_action(&issue_detail_app(), k(KeyCode::Esc)),
            Some(Action::Back)
        );
    }

    #[test]
    fn universal_keys_fire_in_issue_detail() {
        assert_eq!(
            key_to_action(&issue_detail_app(), ch('q')),
            Some(Action::Quit)
        );
        assert_eq!(
            key_to_action(&issue_detail_app(), ch('r')),
            Some(Action::Refresh)
        );
        assert_eq!(
            key_to_action(&issue_detail_app(), ch('?')),
            Some(Action::ToggleHelp)
        );
    }
}
