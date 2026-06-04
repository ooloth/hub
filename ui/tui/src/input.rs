use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::display::{Category, SelectedItemKind};
use tui_input::InputRequest;
use workflows::status::StatusItem;

use crate::state::{Action, App, ReviewSkill, Screen};

pub(crate) fn key_to_action(app: &App, key: KeyEvent) -> Option<Action> {
    // Review picker submenu intercepts all keys while pending.
    if app.ui.pending_review_action {
        return review_picker_submenu_key(key);
    }

    // PR actions submenu intercepts all keys while pending.
    if app.ui.pending_pr_action {
        return pr_action_submenu_key(key);
    }

    // Task status submenu intercepts all keys while pending.
    if app.ui.pending_task_status {
        let current_status = app
            .current_screen()
            .selected_status_item()
            .and_then(|item| {
                if let StatusItem::AgentSession(task) = item {
                    Some(task.status)
                } else {
                    None
                }
            });
        return task_status_submenu_key(key, current_status);
    }

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

    let can_go_back = matches!(
        &app.ui.screen,
        Screen::UnifiedList {
            detail_mode: crate::state::DetailMode::Visible { .. },
            ..
        }
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
        Screen::UnifiedList { detail_mode, .. } => {
            let item_kind = app
                .current_screen()
                .selected_status_item()
                .map(|item| SelectedItemKind::from_item(&item))
                .unwrap_or(SelectedItemKind::Other);
            unified_list_keys(key, detail_mode, item_kind)
        }
        Screen::MergingPr { .. } => unreachable!("handled above"),
        Screen::DismissingIssue { .. } => unreachable!("handled above"),
    }
}

fn pr_action_submenu_key(key: KeyEvent) -> Option<Action> {
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('c'), KeyModifiers::CONTROL)
    ) {
        return Some(Action::Quit);
    }
    match key.code {
        KeyCode::Char('d') => Some(Action::OpenPrDiffInDelta),
        KeyCode::Char('l') => Some(Action::OpenInLazygit),
        KeyCode::Char('o') => Some(Action::OpenInOcto),
        // Esc and any unrecognized key dismiss the submenu without closing the split view.
        _ => Some(Action::CancelPrSubmenu),
    }
}

fn task_status_submenu_key(
    key: KeyEvent,
    current_status: Option<domain::TaskStatus>,
) -> Option<Action> {
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('c'), KeyModifiers::CONTROL)
    ) {
        return Some(Action::Quit);
    }
    let Some(status) = current_status else {
        return Some(Action::CancelTaskStatusSubmenu);
    };
    match (status, key.code) {
        (domain::TaskStatus::Backlog, KeyCode::Char('r')) => {
            Some(Action::TransitionTaskStatus(domain::TaskStatus::Ready))
        }
        (domain::TaskStatus::Backlog, KeyCode::Char('a')) => {
            Some(Action::TransitionTaskStatus(domain::TaskStatus::Archived))
        }
        (domain::TaskStatus::Ready, KeyCode::Char('b')) => {
            Some(Action::TransitionTaskStatus(domain::TaskStatus::Backlog))
        }
        (domain::TaskStatus::Ready, KeyCode::Char('a')) => {
            Some(Action::TransitionTaskStatus(domain::TaskStatus::Archived))
        }
        (domain::TaskStatus::Review, KeyCode::Char('d')) => {
            Some(Action::TransitionTaskStatus(domain::TaskStatus::Done))
        }
        (domain::TaskStatus::Review, KeyCode::Char('r')) => {
            Some(Action::TransitionTaskStatus(domain::TaskStatus::Ready))
        }
        _ => Some(Action::CancelTaskStatusSubmenu),
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

fn unified_list_keys(
    key: KeyEvent,
    detail_mode: &crate::state::DetailMode,
    item_kind: SelectedItemKind,
) -> Option<Action> {
    let split_active = matches!(detail_mode, crate::state::DetailMode::Visible { .. });
    match (key.code, key.modifiers) {
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => Some(Action::MoveUp),
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => Some(Action::MoveDown),
        (KeyCode::Char('J'), _) if split_active => Some(Action::ScrollDetailDown),
        (KeyCode::Char('K'), _) if split_active => Some(Action::ScrollDetailUp),
        // PR-specific actions available when split view is showing a PR.
        (KeyCode::Char('v'), _) if split_active && item_kind == SelectedItemKind::Pr => {
            Some(Action::OpenReviewPicker)
        }
        (KeyCode::Char('m'), _) if split_active && item_kind == SelectedItemKind::Pr => {
            Some(Action::MergePr)
        }
        // d arms the PR diff submenu when split view shows a PR.
        (KeyCode::Char('d'), _) if split_active && item_kind == SelectedItemKind::Pr => {
            Some(Action::PrActionSubmenu)
        }
        // Issue-specific actions available when split view is showing an issue.
        (KeyCode::Char('a'), _) if split_active && item_kind == SelectedItemKind::Issue => {
            Some(Action::ApproveForAgent)
        }
        (KeyCode::Char('w'), _) if split_active && item_kind == SelectedItemKind::Issue => {
            Some(Action::DismissIssue)
        }
        // Task-specific actions available when split view is showing a task.
        (KeyCode::Char('s'), _) if split_active && item_kind == SelectedItemKind::Task => {
            Some(Action::TaskStatusSubmenu)
        }
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
        (KeyCode::Char('o'), _) => Some(Action::OpenUrl),
        (KeyCode::Char('O'), _) => Some(Action::FilterCategory(Category::Issues)),
        (KeyCode::Char('t'), _) => Some(Action::FilterCategory(Category::Tasks)),
        (KeyCode::Char('a'), _) => Some(Action::ClearFilter),
        (KeyCode::Char('/'), _) => Some(Action::StartQuery),
        _ => None,
    }
}

fn review_picker_submenu_key(key: KeyEvent) -> Option<Action> {
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('c'), KeyModifiers::CONTROL)
    ) {
        return Some(Action::Quit);
    }
    match key.code {
        KeyCode::Char('c') => Some(Action::CommitReview(ReviewSkill::Converge)),
        KeyCode::Char('m') => Some(Action::CommitReview(ReviewSkill::PrCommentsConverge)),
        // Esc and any unrecognized key dismiss the submenu without closing the split view.
        _ => Some(Action::CancelReview),
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
    use crate::state::{Action, App, DetailMode, PrPrevScreen, ReviewSkill, Screen, UiState};
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
                    detail_mode: DetailMode::Hidden,
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
    #[case(ch('o'), Some(Action::OpenUrl))]
    #[case(ch('O'), Some(Action::FilterCategory(Category::Issues)))]
    #[case(ch('t'), Some(Action::FilterCategory(Category::Tasks)))]
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

    fn dismissing_app() -> App {
        let parent = ListSnapshot {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: std::collections::HashSet::new(),
            detail_mode: crate::state::DetailMode::Hidden,
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

    fn merging_app() -> App {
        let parent = ListSnapshot {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: std::collections::HashSet::new(),
            detail_mode: crate::state::DetailMode::Hidden,
        };
        let snapshot = parent.clone();
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
                        merge_blocker: None,
                    },
                    prev: PrPrevScreen::UnifiedList { snapshot },
                },
                ..UiState::default()
            },
            ..App::default()
        }
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

    fn pending_review_action_app_with_pr() -> App {
        let mut app = split_view_app_with_pr();
        app.ui.pending_review_action = true;
        app
    }

    #[rstest]
    #[case(ch('c'), Some(Action::CommitReview(ReviewSkill::Converge)))]
    #[case(ch('m'), Some(Action::CommitReview(ReviewSkill::PrCommentsConverge)))]
    #[case(k(KeyCode::Esc), Some(Action::CancelReview))]
    #[case(ch('j'), Some(Action::CancelReview))]
    #[case(ch('k'), Some(Action::CancelReview))]
    #[case(ch('v'), Some(Action::CancelReview))]
    #[case(ch('x'), Some(Action::CancelReview))]
    fn review_picker_submenu_keys(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(
            key_to_action(&pending_review_action_app_with_pr(), key),
            expected
        );
    }

    #[test]
    fn ctrl_c_quits_during_review_picker_submenu() {
        assert_eq!(
            key_to_action(&pending_review_action_app_with_pr(), ctrl('c')),
            Some(Action::Quit)
        );
    }

    // --- Split view context dispatch ---

    fn split_view_app_with_pr() -> App {
        use crate::display::{flatten, DisplayItem};
        use workflows::status::StatusItem;
        let item = StatusItem::Pr(domain::PullRequest {
            number: 1,
            title: "Fix".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/1".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            kind: domain::PrKind::Mine,
            author: "ooloth".to_string(),
            review_decision: None,
            approval_count: 0,
            comment_count: 0,
            head_branch: "feat/fix".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: None,
        });
        let items = vec![DisplayItem::Single(item)];
        let expanded = std::collections::HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    flat_rows,
                    items,
                    selected: 0,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                    detail_mode: crate::state::DetailMode::Visible { detail_scroll: 0 },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn split_view_app_with_issue() -> App {
        use crate::display::{flatten, DisplayItem};
        use workflows::status::StatusItem;
        let item = StatusItem::Issue(domain::Issue {
            number: 42,
            title: "Bug".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/issues/42".to_string(),
            author: "agent".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            labels: vec![],
            body: None,
        });
        let items = vec![DisplayItem::Single(item)];
        let expanded = std::collections::HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    flat_rows,
                    items,
                    selected: 0,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                    detail_mode: crate::state::DetailMode::Visible { detail_scroll: 0 },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn pending_pr_action_app_with_pr() -> App {
        let mut app = split_view_app_with_pr();
        app.ui.pending_pr_action = true;
        app
    }

    // K3: d on PR in split view → PendingPrAction
    #[test]
    fn d_on_pr_in_split_view_arms_pr_submenu() {
        assert_eq!(
            key_to_action(&split_view_app_with_pr(), ch('d')),
            Some(Action::PrActionSubmenu)
        );
    }

    // K4: d on non-PR in split view → nothing
    #[test]
    fn d_on_non_pr_in_split_view_does_nothing() {
        assert_eq!(key_to_action(&split_view_app_with_issue(), ch('d')), None);
    }

    // K5: d→d while pending → OpenPrDiffInDelta
    #[test]
    fn d_while_pending_pr_action_opens_diff() {
        assert_eq!(
            key_to_action(&pending_pr_action_app_with_pr(), ch('d')),
            Some(Action::OpenPrDiffInDelta)
        );
    }

    // K6: d→l while pending → OpenInLazygit
    #[test]
    fn l_while_pending_pr_action_opens_lazygit() {
        assert_eq!(
            key_to_action(&pending_pr_action_app_with_pr(), ch('l')),
            Some(Action::OpenInLazygit)
        );
    }

    // K7: d→o while pending → OpenInOcto
    #[test]
    fn o_while_pending_pr_action_opens_octo() {
        assert_eq!(
            key_to_action(&pending_pr_action_app_with_pr(), ch('o')),
            Some(Action::OpenInOcto)
        );
    }

    // K8: Esc while pending → CancelPrSubmenu (dismisses submenu, keeps split view)
    #[test]
    fn esc_while_pending_pr_action_cancels_submenu_only() {
        assert_eq!(
            key_to_action(&pending_pr_action_app_with_pr(), k(KeyCode::Esc)),
            Some(Action::CancelPrSubmenu)
        );
    }

    // K9: v on PR in split view → OpenReviewPicker
    #[test]
    fn v_on_pr_in_split_view_opens_review_picker() {
        assert_eq!(
            key_to_action(&split_view_app_with_pr(), ch('v')),
            Some(Action::OpenReviewPicker)
        );
    }

    // K10: m on PR in split view → MergePr
    #[test]
    fn m_on_pr_in_split_view_merges_pr() {
        assert_eq!(
            key_to_action(&split_view_app_with_pr(), ch('m')),
            Some(Action::MergePr)
        );
    }

    // K11: a on issue in split view → ApproveForAgent
    #[test]
    fn a_on_issue_in_split_view_approves_for_agent() {
        assert_eq!(
            key_to_action(&split_view_app_with_issue(), ch('a')),
            Some(Action::ApproveForAgent)
        );
    }

    // K12: a on PR in split view → ClearFilter (not ApproveForAgent)
    #[test]
    fn a_on_pr_in_split_view_clears_filter() {
        assert_eq!(
            key_to_action(&split_view_app_with_pr(), ch('a')),
            Some(Action::ClearFilter)
        );
    }

    // K13: w on issue in split view → DismissIssue
    #[test]
    fn w_on_issue_in_split_view_dismisses_issue() {
        assert_eq!(
            key_to_action(&split_view_app_with_issue(), ch('w')),
            Some(Action::DismissIssue)
        );
    }

    fn split_view_app_with_task(status: domain::TaskStatus) -> App {
        use crate::display::{flatten, DisplayItem};
        let urgency = status.urgency();
        let item = workflows::status::StatusItem::AgentSession(domain::Task {
            id: "TASK-0001".parse().unwrap(),
            title: "Fix auth bug".to_string(),
            description: None,
            status,
            kind: domain::TaskKind::General,
            session_id: None,
            issue_links: vec![],
            pr_links: vec![],
            doc_links: vec![],
            created_at: String::new(),
            updated_at: String::new(),
            age: chrono::Duration::zero(),
            urgency,
            comments: vec![],
        });
        let items = vec![DisplayItem::Single(item)];
        let expanded = std::collections::HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    flat_rows,
                    items,
                    selected: 0,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                    detail_mode: crate::state::DetailMode::Visible { detail_scroll: 0 },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn pending_task_status_app(status: domain::TaskStatus) -> App {
        let mut app = split_view_app_with_task(status);
        app.ui.pending_task_status = true;
        app
    }

    // K14: s on task in split view → TaskStatusSubmenu
    #[test]
    fn s_on_task_in_split_view_arms_task_status_submenu() {
        assert_eq!(
            key_to_action(
                &split_view_app_with_task(domain::TaskStatus::Backlog),
                ch('s')
            ),
            Some(Action::TaskStatusSubmenu)
        );
    }

    // K15: s on non-task in split view does nothing
    #[test]
    fn s_on_pr_in_split_view_does_nothing() {
        assert_eq!(key_to_action(&split_view_app_with_pr(), ch('s')), None);
    }

    // K16–K21: task status submenu key mappings per status
    #[rstest]
    #[case(
        domain::TaskStatus::Backlog,
        ch('r'),
        Some(Action::TransitionTaskStatus(domain::TaskStatus::Ready))
    )]
    #[case(
        domain::TaskStatus::Backlog,
        ch('a'),
        Some(Action::TransitionTaskStatus(domain::TaskStatus::Archived))
    )]
    #[case(
        domain::TaskStatus::Ready,
        ch('b'),
        Some(Action::TransitionTaskStatus(domain::TaskStatus::Backlog))
    )]
    #[case(
        domain::TaskStatus::Ready,
        ch('a'),
        Some(Action::TransitionTaskStatus(domain::TaskStatus::Archived))
    )]
    #[case(
        domain::TaskStatus::Review,
        ch('d'),
        Some(Action::TransitionTaskStatus(domain::TaskStatus::Done))
    )]
    #[case(
        domain::TaskStatus::Review,
        ch('r'),
        Some(Action::TransitionTaskStatus(domain::TaskStatus::Ready))
    )]
    fn task_status_submenu_keys_map_correctly(
        #[case] status: domain::TaskStatus,
        #[case] key: KeyEvent,
        #[case] expected: Option<Action>,
    ) {
        assert_eq!(
            key_to_action(&pending_task_status_app(status), key),
            expected
        );
    }

    // K22: unrecognized key in task status submenu → cancel
    #[test]
    fn unrecognized_key_in_task_status_submenu_cancels() {
        assert_eq!(
            key_to_action(
                &pending_task_status_app(domain::TaskStatus::Backlog),
                ch('x')
            ),
            Some(Action::CancelTaskStatusSubmenu)
        );
    }

    // K23: Esc in task status submenu → cancel
    #[test]
    fn esc_in_task_status_submenu_cancels() {
        assert_eq!(
            key_to_action(
                &pending_task_status_app(domain::TaskStatus::Backlog),
                k(KeyCode::Esc)
            ),
            Some(Action::CancelTaskStatusSubmenu)
        );
    }

    // K24: Ctrl-C in task status submenu → Quit
    #[test]
    fn ctrl_c_quits_during_task_status_submenu() {
        assert_eq!(
            key_to_action(
                &pending_task_status_app(domain::TaskStatus::Backlog),
                ctrl('c')
            ),
            Some(Action::Quit)
        );
    }
}
