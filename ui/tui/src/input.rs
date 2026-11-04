use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::display::{Category, SelectedItemKind};
use crate::state::{Action, App, PrAuthor, Screen, SubmenuState};

pub(crate) fn key_to_action(app: &App, key: KeyEvent) -> Option<Action> {
    match &app.ui.submenu {
        SubmenuState::ReviewPicker(target) => {
            return Some(review_picker_submenu_key(key, target.author))
        }
        SubmenuState::PrActions => return Some(pr_action_submenu_key(key)),
        SubmenuState::None => {}
    }

    // Query mode intercepts all keys (Ctrl-C still quits).
    if app.ui.query_input.is_some() {
        return query_mode_key(key);
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
        Screen::MergingPr { .. } => false,
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
            let item_kind = app.current_screen().selected_item_kind();
            unified_list_keys(key, detail_mode, item_kind)
        }
        Screen::MergingPr { .. } => unreachable!("handled above"),
    }
}

const fn pr_action_submenu_key(key: KeyEvent) -> Action {
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('c'), KeyModifiers::CONTROL)
    ) {
        return Action::Quit;
    }
    match key.code {
        KeyCode::Char('d') => Action::OpenPrDiffInDelta,
        KeyCode::Char('l') => Action::OpenInLazygit,
        KeyCode::Char('o') => Action::OpenInOcto,
        // Esc and any unrecognized key dismiss the submenu without closing the split view.
        _ => Action::CancelPrSubmenu,
    }
}

const fn query_mode_key(key: KeyEvent) -> Option<Action> {
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
        (KeyCode::Up | KeyCode::Char('k'), _) => Some(Action::MoveUp),
        (KeyCode::Down | KeyCode::Char('j'), _) => Some(Action::MoveDown),
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
        (KeyCode::Char('a'), _) => Some(Action::ClearFilter),
        (KeyCode::Char('/'), _) => Some(Action::StartQuery),
        _ => None,
    }
}

/// Resolves a keypress against the reviews this author's PRs offer.
///
/// The option table is the same one the status bar renders from, so a key the
/// bar advertises always resolves here and a key it does not advertise never
/// does.
fn review_picker_submenu_key(key: KeyEvent, author: PrAuthor) -> Action {
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('c'), KeyModifiers::CONTROL)
    ) {
        return Action::Quit;
    }
    if let KeyCode::Char(pressed) = key.code {
        if let Some(option) = author
            .review_options()
            .iter()
            .find(|option| option.key == pressed)
        {
            return Action::CommitReview(option.review);
        }
    }
    // Esc, and any key this author has no review for, dismiss the submenu
    // without closing the split view.
    Action::CancelReview
}

const fn merge_confirm_key(key: KeyEvent) -> Option<Action> {
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

#[cfg(test)]
mod tests {
    use super::key_to_action;
    use crate::display::{flatten, Category, DisplayItem, Filter, ListSnapshot};
    use crate::state::{
        Action, App, DetailMode, PrAuthor, PrPrevScreen, PrReview, PrReviewTarget, Screen,
        SubmenuState, UiState,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rstest::rstest;
    use workflows::status::StatusItem;

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

    fn merging_app() -> App {
        let parent = ListSnapshot {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: std::collections::HashSet::new(),
            detail_mode: DetailMode::Hidden,
        };
        let snapshot = parent.clone();
        App {
            ui: UiState {
                screen: Screen::MergingPr {
                    parent,
                    pr: crate::test_support::pull_request(),
                    prev: PrPrevScreen::UnifiedList { snapshot },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    #[rstest]
    #[case(ctrl('c'), Some(Action::Quit))]
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

    fn picker_app(author: PrAuthor) -> App {
        let app = split_view_app_with_pr();
        App {
            ui: UiState {
                submenu: SubmenuState::ReviewPicker(PrReviewTarget {
                    repo: "owner/repo".to_string(),
                    number: 1,
                    head_branch: "feature".to_string(),
                    author,
                }),
                ..app.ui
            },
            ..app
        }
    }

    #[rstest]
    #[case(PrAuthor::Me, ch('c'), Action::CommitReview(PrReview::ReviewMine))]
    #[case(PrAuthor::Me, ch('f'), Action::CommitReview(PrReview::FixMine))]
    #[case(PrAuthor::Me, ch('m'), Action::CommitReview(PrReview::AnswerReviewers))]
    #[case(PrAuthor::Peer, ch('c'), Action::CommitReview(PrReview::ReviewPeer))]
    // A peer's PR offers no review that edits their branch, so the keys that
    // would launch one on my own PR cancel instead.
    #[case(PrAuthor::Peer, ch('f'), Action::CancelReview)]
    #[case(PrAuthor::Peer, ch('m'), Action::CancelReview)]
    #[case(PrAuthor::Me, k(KeyCode::Esc), Action::CancelReview)]
    #[case(PrAuthor::Peer, k(KeyCode::Esc), Action::CancelReview)]
    #[case(PrAuthor::Me, ch('j'), Action::CancelReview)]
    #[case(PrAuthor::Me, ch('v'), Action::CancelReview)]
    #[case(PrAuthor::Peer, ch('x'), Action::CancelReview)]
    fn review_picker_submenu_keys(
        #[case] author: PrAuthor,
        #[case] key: KeyEvent,
        #[case] expected: Action,
    ) {
        assert_eq!(key_to_action(&picker_app(author), key), Some(expected));
    }

    /// The status bar renders its options from `review_options`, so every key it
    /// advertises has to launch a review and every other key has to cancel.
    /// This is what stops the bar and the handler drifting apart.
    #[rstest]
    #[case(PrAuthor::Me)]
    #[case(PrAuthor::Peer)]
    fn the_picker_accepts_exactly_the_keys_it_advertises(#[case] author: PrAuthor) {
        let advertised: Vec<char> = author.review_options().iter().map(|o| o.key).collect();

        for option in author.review_options() {
            assert_eq!(
                key_to_action(&picker_app(author), ch(option.key)),
                Some(Action::CommitReview(option.review)),
                "advertised key {} did not launch its review",
                option.key
            );
        }

        for candidate in 'a'..='z' {
            if advertised.contains(&candidate) {
                continue;
            }
            assert_eq!(
                key_to_action(&picker_app(author), ch(candidate)),
                Some(Action::CancelReview),
                "unadvertised key {candidate} was not a cancel"
            );
        }
    }

    #[rstest]
    #[case(PrAuthor::Me)]
    #[case(PrAuthor::Peer)]
    fn ctrl_c_quits_during_review_picker_submenu(#[case] author: PrAuthor) {
        assert_eq!(
            key_to_action(&picker_app(author), ctrl('c')),
            Some(Action::Quit)
        );
    }

    // --- Split view context dispatch ---

    /// The split view with `item` selected and the detail pane showing.
    fn split_view_app(item: StatusItem) -> App {
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
                    detail_mode: DetailMode::Visible { detail_scroll: 0 },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn split_view_app_with_pr() -> App {
        split_view_app(StatusItem::Pr(crate::test_support::pull_request()))
    }

    fn split_view_app_with_issue() -> App {
        split_view_app(StatusItem::Issue(domain::Issue {
            number: 42,
            title: "Bug".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/issues/42".to_string(),
            author: "agent".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            labels: vec![],
            body: None,
        }))
    }

    fn pending_pr_action_app_with_pr() -> App {
        let app = split_view_app_with_pr();
        App {
            ui: UiState {
                submenu: SubmenuState::PrActions,
                ..app.ui
            },
            ..app
        }
    }

    #[rstest]
    #[case::d_arms_the_pr_submenu(ch('d'), Some(Action::PrActionSubmenu))]
    #[case::v_opens_the_review_picker(ch('v'), Some(Action::OpenReviewPicker))]
    #[case::m_merges(ch('m'), Some(Action::MergePr))]
    // `a` approves only an issue, so on a PR it keeps its list meaning.
    #[case::a_clears_the_filter(ch('a'), Some(Action::ClearFilter))]
    fn split_view_keys_on_a_pr(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&split_view_app_with_pr(), key), expected);
    }

    #[rstest]
    // The PR submenu needs a PR, so `d` on an issue does nothing.
    #[case::d_does_nothing(ch('d'), None)]
    #[case::a_approves_for_the_agent(ch('a'), Some(Action::ApproveForAgent))]
    fn split_view_keys_on_an_issue(#[case] key: KeyEvent, #[case] expected: Option<Action>) {
        assert_eq!(key_to_action(&split_view_app_with_issue(), key), expected);
    }

    #[rstest]
    #[case::d_opens_the_diff(ch('d'), Action::OpenPrDiffInDelta)]
    #[case::l_opens_lazygit(ch('l'), Action::OpenInLazygit)]
    #[case::o_opens_octo(ch('o'), Action::OpenInOcto)]
    // Esc dismisses the submenu and keeps the split view.
    #[case::esc_cancels_only_the_submenu(k(KeyCode::Esc), Action::CancelPrSubmenu)]
    fn pr_submenu_keys(#[case] key: KeyEvent, #[case] expected: Action) {
        assert_eq!(
            key_to_action(&pending_pr_action_app_with_pr(), key),
            Some(expected)
        );
    }
}
