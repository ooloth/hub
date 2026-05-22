use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::display::Category;
use tui_input::InputRequest;

use crate::state::{Action, App, ReviewSkill, Screen};

pub(crate) fn key_to_action(app: &App, key: KeyEvent) -> Option<Action> {
    // Query mode intercepts all keys (Ctrl-C still quits).
    if app.ui.query_input.is_some() {
        return query_mode_key(key);
    }

    // Dismiss prompt intercepts all keys (Ctrl-C still quits).
    if matches!(app.ui.screen, Screen::DismissingIssue { .. }) {
        return dismiss_mode_key(key);
    }

    // Merge confirmation intercepts all keys (Ctrl-C still quits).
    if matches!(app.ui.screen, Screen::MergingPr { .. }) {
        return merge_confirm_key(key);
    }

    // Review picker intercepts all keys (Ctrl-C still quits).
    if matches!(app.ui.screen, Screen::ReviewingPr { .. }) {
        return reviewing_pr_key(key);
    }

    let can_go_back = matches!(
        app.ui.screen,
        Screen::LogDetail { .. } | Screen::IssueDetail { .. } | Screen::PrDetail { .. }
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
        Screen::LogDetail { .. } => log_detail_keys(key),
        Screen::IssueDetail { .. } => issue_reader_keys(key),
        Screen::PrDetail { .. } => pr_reader_keys(key),
        Screen::ReviewingPr { .. } => unreachable!("handled above"),
        Screen::MergingPr { .. } => unreachable!("handled above"),
        Screen::DismissingIssue { .. } => unreachable!("handled above"),
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
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => Some(Action::MoveUp),
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => Some(Action::MoveDown),
        (KeyCode::Char('h'), _) => Some(Action::CollapseGroup),
        (KeyCode::Char('l'), _) => Some(Action::ExpandGroup),
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

fn pr_reader_keys(key: KeyEvent) -> Option<Action> {
    match (key.code, key.modifiers) {
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => Some(Action::MoveUp),
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => Some(Action::MoveDown),
        (KeyCode::Char('g'), _) => Some(Action::PendingG),
        (KeyCode::Char('G'), _) => Some(Action::MoveToBottom),
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => Some(Action::MovePageUp),
        (KeyCode::Char('d'), KeyModifiers::CONTROL) => Some(Action::MovePageDown),
        (KeyCode::Enter, _) => Some(Action::Enter),
        (KeyCode::Char('i'), _) => Some(Action::AskAboutPr),
        (KeyCode::Char('v'), _) => Some(Action::OpenReviewPicker),
        (KeyCode::Char('m'), _) => Some(Action::MergePr),
        _ => None,
    }
}

fn reviewing_pr_key(key: KeyEvent) -> Option<Action> {
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('c'), KeyModifiers::CONTROL)
    ) {
        return Some(Action::Quit);
    }
    match key.code {
        KeyCode::Char('c') => Some(Action::CommitReview(ReviewSkill::Converge)),
        KeyCode::Char('m') => Some(Action::CommitReview(ReviewSkill::PrCommentsConverge)),
        KeyCode::Esc => Some(Action::CancelReview),
        _ => None,
    }
}

fn log_detail_keys(key: KeyEvent) -> Option<Action> {
    match (key.code, key.modifiers) {
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => Some(Action::MoveUp),
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => Some(Action::MoveDown),
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
        (KeyCode::Char('d'), _) => Some(Action::DismissIssue),
        (KeyCode::Char('i'), _) => Some(Action::Investigate),
        _ => None,
    }
}

fn merge_confirm_key(key: KeyEvent) -> Option<Action> {
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('c'), KeyModifiers::CONTROL)
    ) {
        return Some(Action::Quit);
    }
    match key.code {
        KeyCode::Esc => Some(Action::CancelMerge),
        KeyCode::Enter => Some(Action::CommitMerge),
        _ => None,
    }
}

fn dismiss_mode_key(key: KeyEvent) -> Option<Action> {
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('c'), KeyModifiers::CONTROL)
    ) {
        return Some(Action::Quit);
    }
    match key.code {
        KeyCode::Esc => Some(Action::CancelDismissal),
        KeyCode::Enter => Some(Action::CommitDismissal),
        KeyCode::Char(c) => Some(Action::DismissInput(InputRequest::InsertChar(c))),
        KeyCode::Backspace => Some(Action::DismissInput(InputRequest::DeletePrevChar)),
        KeyCode::Delete => Some(Action::DismissInput(InputRequest::DeleteNextChar)),
        KeyCode::Left => Some(Action::DismissInput(InputRequest::GoToPrevChar)),
        KeyCode::Right => Some(Action::DismissInput(InputRequest::GoToNextChar)),
        KeyCode::Home => Some(Action::DismissInput(InputRequest::GoToStart)),
        KeyCode::End => Some(Action::DismissInput(InputRequest::GoToEnd)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::key_to_action;
    use crate::display::{Category, Filter, ListSnapshot};
    use crate::state::{Action, App, ReviewSkill, Screen, UiState};
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

    fn log_detail_app() -> App {
        let snapshot = ListSnapshot {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: std::collections::HashSet::new(),
        };
        App {
            ui: UiState {
                screen: Screen::LogDetail {
                    parent: snapshot,
                    view: crate::display::LogDetailView::Gcp {
                        project: "proj".to_string(),
                        env: "prod".to_string(),
                        title: "errors".to_string(),
                        message: "oops".to_string(),
                        url: String::new(),
                        lookback: "1h".to_string(),
                        lines: vec![crate::display::LogLine::parse("{}")],
                    },
                    scroll: 0,
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
                    flat_rows: vec![],
                    selected: 0,
                    filter: Filter {
                        category: Some(category),
                        query: None,
                    },
                    expanded_groups: std::collections::HashSet::new(),
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
            expanded_groups: std::collections::HashSet::new(),
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
                        author: "agent".to_string(),
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
    fn universal_keys_fire_in_log_detail(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&log_detail_app(), key), expected);
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
    fn esc_goes_back_from_log_detail() {
        assert_eq!(
            key_to_action(&log_detail_app(), k(KeyCode::Esc)),
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
    #[case(ch('h'), Some(Action::CollapseGroup))]
    #[case(k(KeyCode::Down), Some(Action::MoveDown))]
    #[case(ch('j'), Some(Action::MoveDown))]
    #[case(ch('l'), Some(Action::ExpandGroup))]
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
    #[case(k(KeyCode::Down), Some(Action::MoveDown))]
    #[case(ch('j'), Some(Action::MoveDown))]
    #[case(ctrl('u'), Some(Action::MovePageUp))]
    #[case(ctrl('d'), Some(Action::MovePageDown))]
    #[case(k(KeyCode::Enter), Some(Action::Enter))]
    #[case(ch('i'), Some(Action::Investigate))]
    #[case(ch('h'), None)]
    #[case(ch('l'), None)]
    #[case(ch('g'), None)]
    #[case(ch('G'), None)]
    #[case(ch('p'), None)]
    #[case(ch('e'), None)]
    #[case(ch('o'), None)]
    #[case(ch('a'), None)]
    #[case(ch('/'), None)]
    #[case(ch('x'), None)]
    fn log_detail_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&log_detail_app(), key), expected);
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
    #[case(ch('d'), Some(Action::DismissIssue))]
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

    fn dismissing_app() -> App {
        let parent = ListSnapshot {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: std::collections::HashSet::new(),
        };
        App {
            ui: UiState {
                screen: Screen::DismissingIssue {
                    parent,
                    issue: domain::Issue {
                        number: 1,
                        title: "test".to_string(),
                        repo: domain::RepoSlug::new("ooloth", "hub"),
                        url: "https://github.com/ooloth/hub/issues/1".to_string(),
                        author: "agent".to_string(),
                        age: chrono::Duration::zero(),
                        urgency: domain::Urgency::Low,
                        labels: vec![],
                        body: None,
                    },
                    input: tui_input::Input::default(),
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    #[rstest]
    #[case(k(KeyCode::Esc), Some(Action::CancelDismissal))]
    #[case(k(KeyCode::Enter), Some(Action::CommitDismissal))]
    #[case(
        ch('x'),
        Some(Action::DismissInput(tui_input::InputRequest::InsertChar('x')))
    )]
    #[case(
        k(KeyCode::Backspace),
        Some(Action::DismissInput(tui_input::InputRequest::DeletePrevChar))
    )]
    #[case(
        k(KeyCode::Delete),
        Some(Action::DismissInput(tui_input::InputRequest::DeleteNextChar))
    )]
    #[case(
        k(KeyCode::Left),
        Some(Action::DismissInput(tui_input::InputRequest::GoToPrevChar))
    )]
    #[case(
        k(KeyCode::Right),
        Some(Action::DismissInput(tui_input::InputRequest::GoToNextChar))
    )]
    #[case(
        k(KeyCode::Home),
        Some(Action::DismissInput(tui_input::InputRequest::GoToStart))
    )]
    #[case(
        k(KeyCode::End),
        Some(Action::DismissInput(tui_input::InputRequest::GoToEnd))
    )]
    fn dismiss_mode_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&dismissing_app(), key), expected);
    }

    #[test]
    fn ctrl_c_quits_during_dismiss_prompt() {
        assert_eq!(
            key_to_action(&dismissing_app(), ctrl('c')),
            Some(Action::Quit)
        );
    }

    #[test]
    fn normal_keys_intercepted_as_chars_in_dismiss_prompt() {
        // 'q' and 'r' are captured as char inserts, not their usual actions
        assert_eq!(
            key_to_action(&dismissing_app(), ch('q')),
            Some(Action::DismissInput(tui_input::InputRequest::InsertChar(
                'q'
            )))
        );
        assert_eq!(
            key_to_action(&dismissing_app(), ch('r')),
            Some(Action::DismissInput(tui_input::InputRequest::InsertChar(
                'r'
            )))
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

    fn pr_detail_app() -> App {
        let parent = ListSnapshot {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: std::collections::HashSet::new(),
        };
        App {
            ui: UiState {
                screen: Screen::PrDetail {
                    parent,
                    pr: domain::PullRequest {
                        number: 7,
                        title: "test pr".to_string(),
                        repo: domain::RepoSlug::new("ooloth", "hub"),
                        url: "https://github.com/ooloth/hub/pull/7".to_string(),
                        age: chrono::Duration::zero(),
                        urgency: domain::Urgency::Low,
                        kind: domain::PrKind::Mine,
                        author: "ooloth".to_string(),
                        review_decision: None,
                        approval_count: 0,
                        comment_count: 0,
                        head_branch: "feat/thing".to_string(),
                        base_branch: "main".to_string(),
                        body: None,
                        ci_status: None,
                        changed_files: vec![],
                        total_changed_files: 0,
                        review_threads: vec![],
                        pr_comments: vec![],
                    },
                    scroll: 0,
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn merging_app() -> App {
        let parent = ListSnapshot {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: std::collections::HashSet::new(),
        };
        App {
            ui: UiState {
                screen: Screen::MergingPr {
                    parent,
                    pr: domain::PullRequest {
                        number: 7,
                        title: "test pr".to_string(),
                        repo: domain::RepoSlug::new("ooloth", "hub"),
                        url: "https://github.com/ooloth/hub/pull/7".to_string(),
                        age: chrono::Duration::zero(),
                        urgency: domain::Urgency::Low,
                        kind: domain::PrKind::Mine,
                        author: "ooloth".to_string(),
                        review_decision: None,
                        approval_count: 0,
                        comment_count: 0,
                        head_branch: "feat/thing".to_string(),
                        base_branch: "main".to_string(),
                        body: None,
                        ci_status: None,
                        changed_files: vec![],
                        total_changed_files: 0,
                        review_threads: vec![],
                        pr_comments: vec![],
                    },
                },
                ..UiState::default()
            },
            ..App::default()
        }
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
    #[case(ch('i'), Some(Action::AskAboutPr))]
    #[case(ch('v'), Some(Action::OpenReviewPicker))]
    #[case(ch('m'), Some(Action::MergePr))]
    #[case(ch('x'), None)]
    fn pr_detail_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&pr_detail_app(), key), expected);
    }

    #[test]
    fn esc_goes_back_from_pr_detail() {
        assert_eq!(
            key_to_action(&pr_detail_app(), k(KeyCode::Esc)),
            Some(Action::Back)
        );
    }

    #[rstest]
    #[case(k(KeyCode::Esc), Some(Action::CancelMerge))]
    #[case(k(KeyCode::Enter), Some(Action::CommitMerge))]
    #[case(ch('j'), None)]
    #[case(ch('k'), None)]
    #[case(ch('m'), None)]
    #[case(ch('q'), None)]
    #[case(ch('r'), None)]
    fn merge_confirm_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&merging_app(), key), expected);
    }

    #[test]
    fn ctrl_c_quits_during_merge_confirm() {
        assert_eq!(key_to_action(&merging_app(), ctrl('c')), Some(Action::Quit));
    }

    fn reviewing_app() -> App {
        let parent = ListSnapshot {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: std::collections::HashSet::new(),
        };
        App {
            ui: UiState {
                screen: Screen::ReviewingPr {
                    parent,
                    pr: domain::PullRequest {
                        number: 7,
                        title: "test pr".to_string(),
                        repo: domain::RepoSlug::new("ooloth", "hub"),
                        url: "https://github.com/ooloth/hub/pull/7".to_string(),
                        age: chrono::Duration::zero(),
                        urgency: domain::Urgency::Low,
                        kind: domain::PrKind::Mine,
                        author: "ooloth".to_string(),
                        review_decision: None,
                        approval_count: 0,
                        comment_count: 0,
                        head_branch: "feat/thing".to_string(),
                        base_branch: "main".to_string(),
                        body: None,
                        ci_status: None,
                        changed_files: vec![],
                        total_changed_files: 0,
                        review_threads: vec![],
                        pr_comments: vec![],
                    },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    #[rstest]
    #[case(ch('c'), Some(Action::CommitReview(ReviewSkill::Converge)))]
    #[case(ch('m'), Some(Action::CommitReview(ReviewSkill::PrCommentsConverge)))]
    #[case(k(KeyCode::Esc), Some(Action::CancelReview))]
    #[case(ch('j'), None)]
    #[case(ch('k'), None)]
    #[case(ch('v'), None)]
    #[case(ch('i'), None)]
    #[case(ch('q'), None)]
    #[case(ch('r'), None)]
    fn reviewing_pr_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&reviewing_app(), key), expected);
    }

    #[test]
    fn ctrl_c_quits_during_review_picker() {
        assert_eq!(
            key_to_action(&reviewing_app(), ctrl('c')),
            Some(Action::Quit)
        );
    }
}
