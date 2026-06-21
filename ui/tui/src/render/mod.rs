mod pr_detail_columns;
mod theme;

pub(crate) mod detail;
pub(crate) mod issue;
pub(crate) mod log;
pub(crate) mod pr;
pub(crate) mod shared;
pub(crate) mod status_bar;
pub(crate) mod task;
pub(crate) mod unified;

pub(crate) use theme::{dim, list_highlight, FOCUS_COLOR, LAVENDER, YELLOW};

use crate::render::shared::{KEYBINDS_LIST, KEYBINDS_PR_READER};

use chrono::Utc;
use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::state::{App, DetailMode, Screen, SubmenuState};

pub(crate) fn render(frame: &mut ratatui::Frame, app: &mut App) {
    let [content_area, bar_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    render_content(frame, app, content_area);
    render_status_bar(frame, app, bar_area);

    if let Some(modal) = &mut app.ui.modal {
        task::render_task_creation_modal(frame, modal);
    }

    if app.ui.show_help {
        render_help_popup(frame, app);
    }
}

fn render_content(frame: &mut ratatui::Frame, app: &mut App, content_area: ratatui::layout::Rect) {
    match &mut app.ui.screen {
        Screen::UnifiedList {
            flat_rows,
            selected,
            filter,
            items,
            detail_mode,
            ..
        } => match detail_mode {
            DetailMode::Hidden => {
                unified::render_unified(
                    frame,
                    flat_rows,
                    *selected,
                    filter,
                    app.ui.query_input.as_deref(),
                    content_area,
                );
            }
            DetailMode::Visible { detail_scroll } => {
                let max_list_height = content_area.height * 30 / 100;

                let [list_area, detail_area] = Layout::vertical([
                    Constraint::Length(unified::unified_list_height(flat_rows, max_list_height)),
                    Constraint::Min(0),
                ])
                .areas(content_area);

                unified::render_unified(
                    frame,
                    flat_rows,
                    *selected,
                    filter,
                    app.ui.query_input.as_deref(),
                    list_area,
                );

                frame.render_widget(Clear, detail_area);

                let selected_row = flat_rows.get(*selected);

                detail::render_split_detail_pane(
                    frame,
                    detail_area,
                    items,
                    selected_row,
                    detail_scroll,
                    unified::filter_chrome_style(filter, app.ui.query_input.as_deref()),
                );
            }
        },
        Screen::MergingPr { pr, .. } => {
            pr::render_pr_detail(frame, pr, &mut 0, content_area, dim());
        }
    }
}

fn render_status_bar(frame: &mut ratatui::Frame, app: &App, bar_area: ratatui::layout::Rect) {
    let right_status =
        status_bar::right_status_text(&app.data.refresh_state, app.data.last_updated, Utc::now());

    let right_width =
        u16::try_from(Span::raw(right_status.as_str()).width() + 1).unwrap_or(u16::MAX);

    let [bar_left, bar_right] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(bar_area);

    render_status_bar_left(frame, app, bar_left);

    frame.render_widget(
        Paragraph::new(format!("{right_status} ")).style(dim()),
        bar_right,
    );
}

fn render_status_bar_left(frame: &mut ratatui::Frame, app: &App, bar_left: ratatui::layout::Rect) {
    if app.ui.submenu == SubmenuState::PrActions {
        let pr_label = app
            .current_screen()
            .selected_status_item()
            .and_then(|item| {
                if let workflows::status::StatusItem::Pr(pr) = item {
                    Some(format!(" PR #{} · diff", pr.number))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| " diff".to_string());

        let line = Line::from(vec![
            Span::styled(pr_label, Style::default().fg(YELLOW)),
            Span::styled("  [d] delta · [l] lazygit · [o] octo · [Esc] cancel", dim()),
        ]);

        frame.render_widget(Paragraph::new(line), bar_left);
    } else if app.ui.submenu == SubmenuState::ReviewPicker {
        let pr_label = app
            .current_screen()
            .selected_status_item()
            .and_then(|item| {
                if let workflows::status::StatusItem::Pr(pr) = item {
                    Some(format!(" Review #{}", pr.number))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| " Review".to_string());

        let line = Line::from(vec![
            Span::styled(pr_label, Style::default().fg(YELLOW)),
            Span::styled("  [c] code · [m] comments · [Esc] cancel", dim()),
        ]);

        frame.render_widget(Paragraph::new(line), bar_left);
    } else if let Screen::MergingPr { pr, .. } = &app.ui.screen {
        let question = format!(" Squash and merge #{} into {}?", pr.number, pr.base_branch);

        let line = Line::from(vec![
            Span::styled(question, Style::default().fg(YELLOW)),
            Span::styled("  [↩] confirm · [Esc] cancel", dim()),
        ]);

        frame.render_widget(Paragraph::new(line), bar_left);
    } else {
        let left = status_bar::status_bar_left(app);
        frame.render_widget(Paragraph::new(format!(" {left}")).style(dim()), bar_left);
    }
}

fn render_help_popup(frame: &mut ratatui::Frame, app: &App) {
    let keybinds = match &app.ui.screen {
        Screen::UnifiedList { .. } => KEYBINDS_LIST,
        Screen::MergingPr { .. } => KEYBINDS_PR_READER,
    };
    let text = crate::render::shared::format_keybinds(keybinds);
    let lines = u16::try_from(keybinds.len()).unwrap_or(u16::MAX);
    let width = u16::try_from(text.lines().map(|l| l.chars().count()).max().unwrap_or(0))
        .unwrap_or(u16::MAX);
    let popup = crate::render::shared::popup_area(frame.area(), lines, width);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text).block(Block::new().title(" Keybinds ").borders(Borders::ALL)),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::{
        render,
        shared::{urgency_color, urgency_style},
        status_bar, unified,
    };
    use crate::display::{flatten, Category, DisplayItem, Filter, FlatRow, GroupKey, ListSnapshot};
    use crate::state::{
        App, DataState, DetailMode, InvestigateAction, RefreshState, Screen, SubmenuState, UiState,
    };
    use chrono::Utc;
    use proptest::proptest;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::Terminal;
    use rstest::rstest;
    use std::collections::HashSet;
    use workflows::status::StatusItem;

    // ── urgency_color / urgency_style ────────────────────────────────────────

    #[rstest]
    #[case(domain::Urgency::Critical, Color::Red)]
    #[case(domain::Urgency::High, Color::Yellow)]
    #[case(domain::Urgency::Medium, Color::Cyan)]
    #[case(domain::Urgency::Low, Color::Blue)]
    fn urgency_color_maps_each_variant(#[case] urgency: domain::Urgency, #[case] expected: Color) {
        assert_eq!(urgency_color(urgency), expected);
    }

    #[rstest]
    #[case(domain::Urgency::Critical)]
    #[case(domain::Urgency::High)]
    #[case(domain::Urgency::Medium)]
    #[case(domain::Urgency::Low)]
    fn urgency_style_fg_matches_urgency_color(#[case] urgency: domain::Urgency) {
        assert_eq!(urgency_style(urgency).fg, Some(urgency_color(urgency)));
    }

    // ── TestBackend helpers ───────────────────────────────────────────────────

    /// Render `app` into a `width × height` buffer and return it.
    fn draw(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let _ = terminal.draw(|frame| render(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// Render `app1`, then `app2` into the same terminal and return the final buffer.
    /// The second draw sees whatever the first draw left in the buffer — this is
    /// what catches stale-character bugs that only appear after state transitions.
    fn draw_two(
        app1: &mut App,
        app2: &mut App,
        width: u16,
        height: u16,
    ) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let _ = terminal.draw(|frame| render(frame, app1)).unwrap();
        let _ = terminal.draw(|frame| render(frame, app2)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// Serialise the entire buffer as a multi-line string (one line per row).
    /// Empty cells become spaces so each line is exactly `buf.area.width` chars wide.
    fn screen_text(buf: &ratatui::buffer::Buffer) -> String {
        let w = buf.area.width as usize;
        let h = buf.area.height as usize;
        (0..h)
            .map(|y| {
                buf.content[y * w..(y + 1) * w]
                    .iter()
                    .map(|cell| {
                        if cell.symbol().is_empty() {
                            " "
                        } else {
                            cell.symbol()
                        }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Extract the last (status bar) row of a buffer as a plain string.
    /// Empty cells are rendered as spaces so the snapshot width is always `buf.area.width`.
    fn status_row(buf: &ratatui::buffer::Buffer) -> String {
        let w = buf.area.width as usize;
        let last_y = (buf.area.height - 1) as usize;
        buf.content[last_y * w..(last_y + 1) * w]
            .iter()
            .map(|cell| {
                if cell.symbol().is_empty() {
                    " "
                } else {
                    cell.symbol()
                }
            })
            .collect()
    }

    fn ci_item() -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("ooloth", "hub"),
            workflow_name: "CI".to_string(),
            job_name: Some("check".to_string()),
            step_name: Some("fmt".to_string()),
            error: None,
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/ooloth/hub/actions/runs/25608685802".to_string(),
        })
    }

    fn partial_app() -> App {
        App {
            data: DataState {
                // Use None for last_updated so the timestamp is deterministic ("unknown").
                refresh_state: RefreshState::Partial(vec!["source-a".to_string()]),
                last_updated: None,
                ..DataState::default()
            },
            ..App::default()
        }
    }

    fn idle_app() -> App {
        App::default() // RefreshState::Idle, last_updated: None → right status = ""
    }

    // ── Two-frame status bar snapshot tests ──────────────────────────────────
    //
    // These catch stale-character bugs that only emerge after the right or left
    // status text changes length between frames — the bug pattern in the wild.

    #[test]
    fn status_bar_right_partial_then_idle() {
        // Frame 1: long right text ("! source-a unreachable (updated unknown)")
        // Frame 2: empty right text (Idle + no timestamp → "")
        // Catches stale characters if the longer text isn't fully overwritten.
        let buf = draw_two(&mut partial_app(), &mut idle_app(), 80, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn status_bar_right_idle_then_partial() {
        // Frame 1: empty right text → Frame 2: long right text.
        // Checks that the longer string doesn't overflow its allocated area.
        let buf = draw_two(&mut idle_app(), &mut partial_app(), 80, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn status_bar_left_group_then_single_item() {
        // Frame 1: group selected → "1/2 · Press ↩ to expand 1 items"
        // Frame 2: single CI item selected → longer left text with ↩ URL and i hint.
        // Catches stale characters if the longer left text isn't fully overwritten.
        let group = DisplayItem::Group {
            label: GroupKey::new("errors".to_string()),
            items: vec![ci_item()],
        };
        let single = DisplayItem::Single(ci_item());

        let mut app1 = unified_list_app(vec![group]);
        let mut app2 = unified_list_app(vec![single]);
        let buf = draw_two(&mut app1, &mut app2, 120, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn status_bar_left_single_item_then_empty() {
        // Frame 1: CI item selected → long left text with ↩ in it.
        // Frame 2: empty list → left and right are both empty.
        // Isolates whether ↩ causes misalignment that persists into the next frame.
        let mut app1 = unified_list_app(vec![DisplayItem::Single(ci_item())]);
        let mut app2 = unified_list_app(vec![]);
        let buf = draw_two(&mut app1, &mut app2, 120, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    // ── Single-frame baseline snapshots ──────────────────────────────────────
    //
    // Capture what the status bar looks like in key states so regressions are
    // visible as snapshot diffs.

    #[test]
    fn status_bar_single_frame_ci_selected() {
        // CI item selected: left shows position + ↩ URL + i hints; right is empty.
        let mut app = unified_list_app(vec![DisplayItem::Single(ci_item())]);
        let buf = draw(&mut app, 120, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn status_bar_single_frame_partial_state() {
        // Partial refresh: left is empty (no items); right shows the warning.
        let mut app = partial_app();
        let buf = draw(&mut app, 80, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn status_bar_single_frame_in_progress() {
        let mut app = App {
            data: DataState {
                refresh_state: RefreshState::InProgress,
                ..DataState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    fn pr() -> StatusItem {
        StatusItem::Pr(domain::PullRequest {
            number: 1,
            title: "Fix".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/1".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            kind: domain::PrKind::ToReview,
            author: "alice".to_string(),
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
        })
    }

    fn unified_list_app(items: Vec<DisplayItem>) -> App {
        let expanded = HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    flat_rows,
                    items,
                    selected: 0,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                    detail_mode: DetailMode::Hidden,
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn make_screen(items: Vec<DisplayItem>, selected: usize, filter: Filter) -> Screen {
        let expanded = HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        Screen::UnifiedList {
            flat_rows,
            items,
            selected,
            filter,
            expanded_groups: expanded,
            detail_mode: DetailMode::Hidden,
        }
    }

    fn expanded_group_app(group_items: Vec<workflows::status::StatusItem>, selected: usize) -> App {
        use crate::display::GroupKey;
        let key = GroupKey::new("hub errors".to_string());
        let items = vec![DisplayItem::Group {
            label: key.clone(),
            items: group_items,
        }];
        let mut expanded = HashSet::new();
        let _ = expanded.insert(key);
        let flat_rows = flatten(&items, &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    flat_rows,
                    items,
                    selected,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                    detail_mode: DetailMode::Hidden,
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    #[test]
    fn status_bar_shows_flash_when_set() {
        let app = App {
            ui: UiState {
                flash: Some("something went wrong".to_string()),
                ..UiState::default()
            },
            ..App::default()
        };
        assert_eq!(status_bar::status_bar_left(&app), "something went wrong");
    }

    #[test]
    fn position_label_unified_list_shows_index_of_n() {
        let items = vec![
            DisplayItem::Single(pr()),
            DisplayItem::Single(pr()),
            DisplayItem::Single(pr()),
        ];
        let expanded = HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        let screen = Screen::UnifiedList {
            flat_rows,
            items,
            selected: 1,
            filter: Filter::default(),
            expanded_groups: expanded,
            detail_mode: DetailMode::Hidden,
        };
        assert_eq!(status_bar::position_label(&screen), "2/3");
    }

    #[test]
    fn investigate_hint_returns_empty_when_none() {
        assert_eq!(status_bar::investigate_hint(&InvestigateAction::None), "");
    }

    #[test]
    fn investigate_hint_returns_hint_when_actionable() {
        let inv = InvestigateAction::LaunchCi {
            repo: "owner/repo".to_string(),
            run_url: "https://example.com".to_string(),
        };
        assert_eq!(status_bar::investigate_hint(&inv), " · [i] investigate");
    }

    #[cfg(feature = "private")]
    #[test]
    fn investigate_hint_returns_hint_for_media() {
        let inv = InvestigateAction::LaunchMediaBlocked {
            title: "Show — S01E01".to_string(),
            error: "Invalid video file".to_string(),
        };
        assert_eq!(status_bar::investigate_hint(&inv), " · [i] investigate");
    }

    #[test]
    fn right_status_in_progress() {
        let now = Utc::now();
        assert_eq!(
            status_bar::right_status_text(&RefreshState::InProgress, None, now),
            "refreshing…"
        );
    }

    #[test]
    fn right_status_failed_shows_error_message() {
        let now = Utc::now();
        assert_eq!(
            status_bar::right_status_text(
                &RefreshState::Failed("network error".to_string()),
                None,
                now
            ),
            "refresh failed: network error"
        );
    }

    #[test]
    fn right_status_idle_no_timestamp_is_empty() {
        let now = Utc::now();
        assert_eq!(
            status_bar::right_status_text(&RefreshState::Idle, None, now),
            ""
        );
    }

    #[test]
    fn right_status_idle_updated_within_a_minute() {
        let now = Utc::now();
        let last_updated = now - chrono::Duration::seconds(30);
        assert_eq!(
            status_bar::right_status_text(&RefreshState::Idle, Some(last_updated), now),
            "updated just now"
        );
    }

    #[test]
    fn right_status_idle_updated_minutes_ago() {
        let now = Utc::now();
        let last_updated = now - chrono::Duration::minutes(5);
        assert_eq!(
            status_bar::right_status_text(&RefreshState::Idle, Some(last_updated), now),
            "updated 5m ago"
        );
    }

    #[test]
    fn right_status_partial_no_timestamp() {
        let now = Utc::now();
        assert_eq!(
            status_bar::right_status_text(
                &RefreshState::Partial(vec!["media".to_string()]),
                None,
                now
            ),
            "! media unreachable (updated unknown)"
        );
    }

    #[test]
    fn right_status_partial_updated_within_a_minute() {
        let now = Utc::now();
        let last_updated = now - chrono::Duration::seconds(10);
        assert_eq!(
            status_bar::right_status_text(
                &RefreshState::Partial(vec!["media".to_string()]),
                Some(last_updated),
                now
            ),
            "! media unreachable (updated just now)"
        );
    }

    #[test]
    fn right_status_partial_multiple_sources() {
        let now = Utc::now();
        let last_updated = now - chrono::Duration::minutes(2);
        assert_eq!(
            status_bar::right_status_text(
                &RefreshState::Partial(vec!["media".to_string(), "linear issues".to_string()]),
                Some(last_updated),
                now
            ),
            "! media, linear issues unreachable (updated 2m ago)"
        );
    }

    // ── Full-screen unified list snapshots ───────────────────────────────────
    //
    // These capture the entire rendered buffer (not just the status bar row) so
    // that regressions in list layout, borders, urgency dividers, and item
    // rendering are visible as snapshot diffs.

    #[test]
    fn full_screen_unified_list_empty() {
        // U1: No items — just the "All" border with an empty body and status bar.
        let mut app = unified_list_app(vec![]);
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_mixed_urgency() {
        // U2: High-urgency CI item + Low-urgency PR → urgency divider between them.
        let mut app = unified_list_app(vec![
            DisplayItem::Single(ci_item()),
            DisplayItem::Single(pr()),
        ]);
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_group_selected() {
        // U3: A group item is selected — shows "↩ to expand" hint in the row.
        let mut app = unified_list_app(vec![DisplayItem::Group {
            label: GroupKey::new("hub errors".to_string()),
            items: vec![ci_item()],
        }]);
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_category_filter() {
        // U4: Category filter active — green border + category label in the title.
        let mut app = App {
            ui: UiState {
                screen: make_screen(
                    vec![DisplayItem::Single(ci_item())],
                    0,
                    Filter {
                        category: Some(Category::Errors),
                        query: None,
                    },
                ),
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_query_input() {
        // U5: Search query being typed — yellow border + query text in the title.
        let mut app = App {
            ui: UiState {
                screen: make_screen(vec![DisplayItem::Single(ci_item())], 0, Filter::default()),
                query_input: Some("hub".to_string()),
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_narrow_terminal() {
        // U6: 40-column terminal — long item text must wrap onto a second line.
        let mut app = unified_list_app(vec![DisplayItem::Single(ci_item())]);
        let buf = draw(&mut app, 40, 10);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_help_popup() {
        // U7: Help popup overlaid on the list.
        let mut app = App {
            ui: UiState {
                screen: make_screen(vec![DisplayItem::Single(ci_item())], 0, Filter::default()),
                show_help: true,
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 20);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // ── More full-screen unified list snapshots ───────────────────────────────

    #[test]
    fn full_screen_unified_list_pr_selected() {
        // U8: PR item (Low urgency, no investigate action) selected as item 2/2.
        // Tests that: (a) the inline hint is "↩ to open" only (no "i" hint);
        // (b) the CI item above it shows no hint; (c) position label is "2/2".
        let mut app = App {
            ui: UiState {
                screen: make_screen(
                    vec![DisplayItem::Single(ci_item()), DisplayItem::Single(pr())],
                    1,
                    Filter::default(),
                ),
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_scrolled() {
        // U9: 15 PR items, last item selected — list must scroll to show it.
        // Tests that the scroll offset logic renders only the visible window
        // and the selected item appears at the bottom of the viewport.
        let items: Vec<DisplayItem> = (0..15).map(|_| DisplayItem::Single(pr())).collect();
        let mut app = App {
            ui: UiState {
                screen: make_screen(items, 14, Filter::default()),
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_empty_filter_result() {
        // U10: Category filter active but no items match — green border with
        // filter title, empty body.  Different from U1 (no filter) and U4 (items match).
        let mut app = App {
            ui: UiState {
                screen: make_screen(
                    vec![],
                    0,
                    Filter {
                        category: Some(Category::Errors),
                        query: None,
                    },
                ),
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_unified_list_committed_query() {
        // U11: Query committed (filter.query set, query_input None) — green
        // border + query in title. Distinct from U5 (query_input set → yellow border).
        let mut app = App {
            ui: UiState {
                screen: make_screen(
                    vec![DisplayItem::Single(ci_item())],
                    0,
                    Filter {
                        category: None,
                        query: Some("hub".to_string()),
                    },
                ),
                ..UiState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // ── Full-screen MergingPr snapshots ──────────────────────────────────────

    fn stub_pr_with_body() -> domain::PullRequest {
        domain::PullRequest {
            number: 102,
            title: "Add PrDetail screen".to_string(),
            repo: domain::RepoSlug::new("ooloth", "hub"),
            url: "https://github.com/ooloth/hub/pull/102".to_string(),
            age: chrono::Duration::days(1),
            urgency: domain::Urgency::Medium,
            kind: domain::PrKind::Mine,
            author: "ooloth".to_string(),
            review_decision: None,
            approval_count: 0,
            comment_count: 2,
            head_branch: "feat/pr-detail".to_string(),
            base_branch: "main".to_string(),
            body: Some(
                "## Summary\n\nAdds a detail screen for PRs.\n\n\
                 ## Why\n\nReadability from terminal without leaving the TUI."
                    .to_string(),
            ),
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: None,
        }
    }

    // ── Full-screen MergingPr snapshots ──────────────────────────────────────

    fn merging_pr_app(pr: domain::PullRequest) -> App {
        let snapshot = ListSnapshot {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: HashSet::new(),
            detail_mode: crate::state::DetailMode::Hidden,
        };
        App {
            ui: UiState {
                screen: Screen::MergingPr {
                    parent: snapshot.clone(),
                    pr,
                    prev: crate::state::PrPrevScreen::UnifiedList { snapshot },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    #[test]
    fn full_screen_merging_pr() {
        // M1: Merge confirmation modal open over PR body.
        let mut app = merging_pr_app(stub_pr_with_body());
        let buf = draw(&mut app, 120, 30);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn status_bar_in_merging_pr() {
        // M2: Status bar shows "[↩] confirm · [Esc] cancel".
        let mut app = merging_pr_app(stub_pr_with_body());
        let buf = draw(&mut app, 120, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    // ── Full-screen detail view snapshots ─────────────────────────────────────

    #[test]
    fn full_screen_detail_view_first_selected() {
        // D1: Group expanded inline, first child selected.
        let mut app = expanded_group_app(vec![ci_item(), ci_item()], 1);
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    #[test]
    fn full_screen_detail_view_last_selected() {
        // D2: Group expanded inline, last child selected.
        let mut app = expanded_group_app(vec![ci_item(), ci_item()], 2);
        let buf = draw(&mut app, 80, 15);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // ── Status bar gap snapshots ──────────────────────────────────────────────

    #[test]
    fn status_bar_single_frame_failed() {
        // S1: Failed refresh — right side shows "refresh failed: …".
        let mut app = App {
            data: DataState {
                refresh_state: RefreshState::Failed("connection refused".to_string()),
                ..DataState::default()
            },
            ..App::default()
        };
        let buf = draw(&mut app, 80, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn status_bar_single_frame_detail_view() {
        // S2: Group expanded inline, child selected — enter hint shows "open" (item has URL).
        let mut app = expanded_group_app(vec![ci_item(), ci_item()], 1);
        let buf = draw(&mut app, 120, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    #[test]
    fn wrap_text_treats_zero_width_as_one_column() {
        assert_eq!(unified::wrap_text("abc", 0), vec!["a", "b", "c"]);
    }

    #[test]
    fn wrap_text_hard_wraps_without_dropping_characters() {
        assert_eq!(unified::wrap_text("abcdef", 3), vec!["abc", "def"]);
    }

    #[test]
    fn wrap_text_prefers_word_boundaries() {
        assert_eq!(
            unified::wrap_text("alpha beta gamma", 10),
            vec!["alpha beta", "gamma"]
        );
    }

    // ── Split view snapshot tests ─────────────────────────────────────────────

    fn split_view_app(items: Vec<DisplayItem>, selected: usize, detail_scroll: u16) -> App {
        let expanded = HashSet::new();
        let flat_rows = flatten(&items, &expanded);
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    flat_rows,
                    items,
                    selected,
                    filter: Filter::default(),
                    expanded_groups: expanded,
                    detail_mode: DetailMode::Visible { detail_scroll },
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn issue_item() -> StatusItem {
        StatusItem::Issue(domain::Issue {
            number: 42,
            title: "Fix logging in production".to_string(),
            repo: domain::RepoSlug::new("ooloth", "hub"),
            url: "https://github.com/ooloth/hub/issues/42".to_string(),
            author: "agent".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            labels: vec!["status:needs-human-review".to_string()],
            body: Some("This is the issue body.\n\nIt has multiple paragraphs.".to_string()),
        })
    }

    // SV1: Split view with a PR selected, scroll 0.
    #[test]
    fn split_view_pr_selected() {
        let mut app = split_view_app(vec![DisplayItem::Single(pr())], 0, 0);
        let buf = draw(&mut app, 120, 30);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // SV2: Split view with an issue selected.
    #[test]
    fn split_view_issue_selected() {
        let mut app = split_view_app(vec![DisplayItem::Single(issue_item())], 0, 0);
        let buf = draw(&mut app, 120, 30);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // SV3: Split view with a CI item selected (no detail view → placeholder).
    #[test]
    fn split_view_placeholder_for_ci_item() {
        let mut app = split_view_app(vec![DisplayItem::Single(ci_item())], 0, 0);
        let buf = draw(&mut app, 120, 30);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // SV4: Split view with detail_scroll > 0.
    #[test]
    fn split_view_pr_scrolled() {
        let mut app = split_view_app(vec![DisplayItem::Single(pr())], 0, 5);
        let buf = draw(&mut app, 120, 30);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // SV5: Split view on a narrow terminal (80 cols).
    #[test]
    fn split_view_narrow_terminal() {
        let mut app = split_view_app(vec![DisplayItem::Single(pr())], 0, 0);
        let buf = draw(&mut app, 80, 30);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // SV6: Fullscreen list after Esc (regression — Hidden mode unchanged).
    #[test]
    fn fullscreen_list_after_esc_from_split_view() {
        let items = vec![DisplayItem::Single(pr()), DisplayItem::Single(ci_item())];
        let mut app = unified_list_app(items);
        let buf = draw(&mut app, 120, 20);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // SL5: PrActions submenu shows in status bar.
    #[test]
    fn status_bar_pending_pr_action_shows_submenu() {
        let mut app = split_view_app(vec![DisplayItem::Single(pr())], 0, 0);
        app.ui.submenu = SubmenuState::PrActions;
        let buf = draw(&mut app, 120, 5);
        insta::assert_snapshot!(status_row(&buf));
    }

    // ── unified_list_height ──────────────────────────────────────────────────

    fn pr_with_urgency(urgency: domain::Urgency) -> StatusItem {
        StatusItem::Pr(domain::PullRequest {
            urgency,
            ..match pr() {
                StatusItem::Pr(p) => p,
                _ => unreachable!(),
            }
        })
    }

    #[test]
    fn unified_list_height_shrinks_to_fit_when_below_cap() {
        let rows = vec![
            FlatRow::Single(pr()),
            FlatRow::Single(pr()),
            FlatRow::Single(pr()),
        ];
        // 3 items + 0 dividers (all Urgency::Low) + 2 borders = 5
        assert_eq!(unified::unified_list_height(&rows, 30), 5);
    }

    #[test]
    fn unified_list_height_clamped_to_max_when_rows_exceed_cap() {
        let rows: Vec<FlatRow> = (0..20).map(|_| FlatRow::Single(pr())).collect();
        // 20 items + 0 dividers + 2 borders = 22, capped at 10
        assert_eq!(unified::unified_list_height(&rows, 10), 10);
    }

    #[test]
    fn unified_list_height_includes_divider_lines() {
        let rows = vec![
            FlatRow::Single(pr_with_urgency(domain::Urgency::High)),
            FlatRow::Single(pr()),
        ];
        // 2 items + 1 divider (High → Low) + 2 borders = 5
        assert_eq!(unified::unified_list_height(&rows, 30), 5);
    }

    #[test]
    fn unified_list_height_counts_multiple_dividers() {
        let rows = vec![
            FlatRow::Single(pr_with_urgency(domain::Urgency::Critical)),
            FlatRow::Single(pr_with_urgency(domain::Urgency::High)),
            FlatRow::Single(pr()),
        ];
        // 3 items + 2 dividers (Critical→High, High→Low) + 2 borders = 7
        assert_eq!(unified::unified_list_height(&rows, 30), 7);
    }

    #[test]
    fn unified_list_height_empty_rows_yields_borders_only() {
        let rows: Vec<FlatRow> = vec![];
        // 0 items + 0 dividers + 2 borders = 2
        assert_eq!(unified::unified_list_height(&rows, 30), 2);
    }

    proptest! {
        #[test]
        fn unified_list_height_never_exceeds_max(
            item_count in 0usize..=1000,
            divider_count in 0usize..=10,
            max_height in 0u16..=200,
        ) {
            let result = unified::unified_list_height_from_counts(item_count, divider_count, max_height);
            assert!(result <= max_height);
        }
    }

    // SV7: Split view with items spanning two urgency tiers (locks in divider-inclusive sizing).
    #[test]
    fn split_view_multi_urgency_tiers() {
        let items = vec![
            DisplayItem::Single(pr_with_urgency(domain::Urgency::High)),
            DisplayItem::Single(pr()),
            DisplayItem::Single(pr()),
        ];
        let mut app = split_view_app(items, 0, 0);
        let buf = draw(&mut app, 120, 40);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // ── Task creation modal snapshots ────────────────────────────────────────

    fn modal_app_blank() -> App {
        use crate::state::TaskCreationModal;
        App {
            ui: UiState {
                modal: Some(TaskCreationModal::blank()),
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn modal_app_submit_focused() -> App {
        use crate::state::{TaskCreationModal, TaskFormField};
        use tui_textarea::TextArea;
        let mut modal = TaskCreationModal::blank();
        modal.title = TextArea::new(vec!["Fix auth bug".to_string()]);
        modal.description = TextArea::new(vec!["Auth is broken in prod".to_string()]);
        modal.kind = domain::TaskKind::Debug;
        modal.link = TextArea::new(vec!["https://github.com/org/repo/issues/1".to_string()]);
        modal.focused_field = TaskFormField::Submit;
        App {
            ui: UiState {
                modal: Some(modal),
                ..UiState::default()
            },
            ..App::default()
        }
    }

    // M1: blank (task_creation,) Title focused — baseline layout snapshot.
    #[test]
    fn task_creation_modal_blank_title_focused() {
        let mut app = modal_app_blank();
        let buf = draw(&mut app, 80, 24);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // M2: all fields populated, Submit focused — "ready to create" state.
    #[test]
    fn task_creation_modal_all_fields_submit_focused() {
        let mut app = modal_app_submit_focused();
        let buf = draw(&mut app, 80, 24);
        insta::assert_snapshot!(screen_text(&buf));
    }

    fn seeded_modal_app(item: workflows::status::StatusItem) -> App {
        use crate::state::{task_creation, TaskCreationModal};
        App {
            ui: UiState {
                modal: Some(TaskCreationModal::with_seed(
                    task_creation::seed_from_item(&item),
                    vec![],
                )),
                ..UiState::default()
            },
            ..App::default()
        }
    }

    // M3: seeded from a CI failure — description renders as separate lines; kind=Debug; link=run URL.
    #[test]
    fn task_creation_modal_seeded_from_ci_row() {
        let item = workflows::status::StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("org", "hub"),
            workflow_name: "CI".to_string(),
            job_name: Some("build".to_string()),
            step_name: Some("run tests".to_string()),
            error: Some("panicked at assertion failed".to_string()),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/org/hub/actions/runs/99".to_string(),
        });
        let buf = draw(&mut seeded_modal_app(item), 80, 24);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // M4: seeded from a PR — kind=Review; description shows author/branch/status.
    #[test]
    fn task_creation_modal_seeded_from_pr_row() {
        let item = workflows::status::StatusItem::Pr(domain::PullRequest {
            number: 42,
            title: "Add dark mode".to_string(),
            repo: domain::RepoSlug::new("org", "hub"),
            url: "https://github.com/org/hub/pull/42".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            kind: domain::PrKind::ToReview,
            author: "alice".to_string(),
            review_decision: Some(domain::ReviewDecision::ChangesRequested),
            approval_count: 0,
            comment_count: 0,
            head_branch: "feat/dark-mode".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: None,
        });
        let buf = draw(&mut seeded_modal_app(item), 80, 24);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // M5: seeded from a GitHub issue — kind=Implement; description shows repo/author.
    #[test]
    fn task_creation_modal_seeded_from_issue_row() {
        let item = workflows::status::StatusItem::Issue(domain::Issue {
            number: 7,
            title: "Button misaligned on mobile".to_string(),
            repo: domain::RepoSlug::new("org", "hub"),
            url: "https://github.com/org/hub/issues/7".to_string(),
            author: "bob".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            labels: vec!["bug".to_string()],
            body: None,
        });
        let buf = draw(&mut seeded_modal_app(item), 80, 24);
        insta::assert_snapshot!(screen_text(&buf));
    }

    // M6: seeded from a Loki alert — kind=Debug; description shows alert/message/line/lookback.
    #[test]
    fn task_creation_modal_seeded_from_loki_row() {
        let item = workflows::status::StatusItem::Loki(domain::LokiEntry {
            title: "High error rate".to_string(),
            project: "myapp".to_string(),
            env: "prod".to_string(),
            message: "connection refused".to_string(),
            line: r#"{"level":"error","msg":"connection refused"}"#.to_string(),
            lookback: "15m".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Critical,
            url: "https://grafana.example.com/d/abc".to_string(),
        });
        let buf = draw(&mut seeded_modal_app(item), 80, 24);
        insta::assert_snapshot!(screen_text(&buf));
    }
}
