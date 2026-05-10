use chrono::Utc;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::display::{display_item_line, display_item_urgency, DisplayItem, Filter};
use crate::state::{
    compute_enter_action, compute_investigate_action, App, EnterAction, InvestigateAction,
    RefreshState, Screen,
};

mod detail;

pub(super) const FOCUS_COLOR: Color = Color::Green;
pub(super) const SELECTION_BG: Color = Color::Rgb(41, 45, 62);

pub(super) fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub(super) fn list_highlight() -> Style {
    Style::default()
        .bg(SELECTION_BG)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn urgency_style(u: domain::Urgency) -> Style {
    match u {
        domain::Urgency::Critical => Style::default().fg(Color::Red),
        domain::Urgency::High => Style::default().fg(Color::Yellow),
        domain::Urgency::Medium => Style::default(),
        domain::Urgency::Low => dim(),
    }
}

const KEYBINDS_LIST: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("h / k", "up"),
    ("j / l", "down"),
    ("Enter", "open / drill into group"),
    ("i", "investigate"),
    ("p / e / o", "filter PRs / Errors / Issues"),
    ("/", "search"),
    ("a / Esc", "clear filter"),
    ("r", "refresh"),
    ("q / Ctrl-C", "quit"),
];

const KEYBINDS_DETAIL: &[(&str, &str)] = &[
    ("?", "toggle help"),
    ("h / k", "up"),
    ("j / l", "down"),
    ("Enter", "open URL"),
    ("i", "investigate"),
    ("r", "refresh"),
    ("Esc", "back to list"),
    ("q / Ctrl-C", "quit"),
];

fn format_keybinds(keybinds: &[(&str, &str)]) -> String {
    let key_w = keybinds
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    keybinds
        .iter()
        .map(|(k, d)| format!("  {k:<key_w$}   {d}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn popup_area(area: Rect, content_lines: u16, content_width: u16) -> Rect {
    let width = (content_width + 4).min(area.width);
    let height = (content_lines + 2).min(area.height);
    Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    )
}

pub(super) fn render_list_view<T>(
    frame: &mut ratatui::Frame,
    area: Rect,
    items: &[T],
    list_state: &mut ListState,
    item_data: impl Fn(&T) -> (String, Option<String>, domain::Urgency),
    hint_fn: impl Fn(&T) -> Option<String>,
) {
    let text_width = area.width.saturating_sub(2) as usize;
    let selected = list_state.selected();
    let selected_hint = selected.and_then(|i| items.get(i)).and_then(hint_fn);
    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = selected == Some(i);
            let (line_text, dim_suffix, urgency) = item_data(item);
            let dot_style = if is_selected {
                Style::default()
            } else {
                urgency_style(urgency)
            };
            let hint = if is_selected {
                selected_hint.clone()
            } else {
                None
            };
            let item_width = text_width
                .saturating_sub(dim_suffix.as_ref().map_or(0, |s| s.chars().count()))
                .saturating_sub(hint.as_ref().map_or(0, |h| h.chars().count() + 2));
            build_list_item(
                Span::styled("● ", dot_style),
                wrap_text(&line_text, item_width),
                dim_suffix,
                hint,
            )
        })
        .collect();
    let list = List::new(list_items).highlight_style(list_highlight());
    frame.render_stateful_widget(list, area, list_state);
}

fn build_list_item(
    dot: Span<'static>,
    wrapped: Vec<String>,
    dim_suffix: Option<String>,
    hint: Option<String>,
) -> ListItem<'static> {
    let suffix_span = dim_suffix.map(|s| Span::styled(s, dim()));
    let hint_span = hint.map(|h| Span::styled(format!("  {h}"), dim()));
    let mut lines: Vec<Line> = wrapped
        .into_iter()
        .enumerate()
        .map(|(j, chunk)| {
            if j == 0 {
                Line::from(vec![dot.clone(), Span::raw(chunk)])
            } else {
                Line::from(Span::raw(format!("  {chunk}")))
            }
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::from(vec![dot, Span::raw("")]));
    }
    if let Some(last) = lines.last_mut() {
        if let Some(s) = suffix_span {
            last.spans.push(s);
        }
        if let Some(h) = hint_span {
            last.spans.push(h);
        }
    }
    ListItem::new(Text::from(lines))
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    if total <= width {
        return vec![text.to_string()];
    }
    let mut lines = vec![];
    let mut start = 0;
    while start < total {
        let end = (start + width).min(total);
        if end == total {
            lines.push(chars[start..].iter().collect());
            break;
        }
        if chars[end] == ' ' {
            lines.push(chars[start..end].iter().collect());
            start = end + 1;
            while start < total && chars[start] == ' ' {
                start += 1;
            }
            continue;
        }
        let split = chars[start..end]
            .iter()
            .rposition(|&c| c == ' ')
            .filter(|&p| p > 0)
            .map(|p| start + p);
        if let Some(split) = split {
            lines.push(chars[start..split].iter().collect());
            start = split + 1;
            while start < total && chars[start] == ' ' {
                start += 1;
            }
        } else {
            lines.push(chars[start..end].iter().collect());
            start = end;
        }
    }
    lines
}

fn position_label(screen: &Screen) -> String {
    match screen {
        Screen::UnifiedList {
            items, selected, ..
        } => {
            let n = items.len();
            if n == 0 {
                String::new()
            } else {
                format!("{}/{n}", selected + 1)
            }
        }
        Screen::Detail { parent, view } => {
            let count = match parent.items.get(view.group_index) {
                Some(DisplayItem::Group { items, .. }) => items.len(),
                _ => 0,
            };
            view.list_state
                .selected()
                .map(|i| format!("{}/{count}", i + 1))
                .unwrap_or_default()
        }
    }
}

fn action_hints(enter: &EnterAction, investigate: &InvestigateAction) -> String {
    let enter_hint = match enter {
        EnterAction::OpenUrl(url) => format!(" · Press ↩ to open {url}"),
        EnterAction::OpenDetail { item_count, .. } => {
            format!(" · Press ↩ to expand ({item_count} items)")
        }
        EnterAction::None => String::new(),
    };
    let inv_hint = if matches!(investigate, InvestigateAction::None) {
        ""
    } else {
        " · Press i to investigate"
    };
    format!("{enter_hint}{inv_hint}")
}

fn status_bar_left(app: &App) -> String {
    if let Some(flash) = &app.ui.flash {
        return flash.clone();
    }
    let enter_action = compute_enter_action(app);
    let investigate_action = compute_investigate_action(app);
    let pos = position_label(app.current_screen());
    let hints = action_hints(&enter_action, &investigate_action);
    format!("{pos}{hints}")
}

fn unified_title(filter: &Filter, query_input: Option<&str>) -> String {
    if let Some(q) = query_input {
        let cat_prefix = filter
            .category
            .map(|c| format!("{} · ", c.label()))
            .unwrap_or_default();
        return format!(" {cat_prefix}\"{q}\" ");
    }
    match (&filter.category, &filter.query) {
        (None, None) => " All ".to_string(),
        (Some(cat), None) => format!(" {} ", cat.label()),
        (None, Some(q)) => format!(" \"{}\" ", q),
        (Some(cat), Some(q)) => format!(" {} · \"{}\" ", cat.label(), q),
    }
}

/// Returns the style for filter-related chrome (border, dividers).
/// Yellow while a query is being typed; green once any filter is committed; dim otherwise.
fn filter_chrome_style(filter: &Filter, query_input: Option<&str>) -> Style {
    if query_input.is_some() {
        Style::default().fg(Color::Yellow)
    } else if !filter.is_empty() {
        Style::default().fg(FOCUS_COLOR)
    } else {
        dim()
    }
}

fn urgency_divider(width: usize, chrome: Style) -> ListItem<'static> {
    let line = "─".repeat(width);
    ListItem::new(Line::from(Span::styled(line, chrome)))
}

fn render_unified(
    frame: &mut ratatui::Frame,
    items: &[DisplayItem],
    selected: usize,
    filter: &Filter,
    query_input: Option<&str>,
    area: Rect,
) {
    let chrome = filter_chrome_style(filter, query_input);
    let title_style = chrome
        .add_modifier(Modifier::BOLD)
        .remove_modifier(Modifier::DIM);
    let title = Span::styled(unified_title(filter, query_input), title_style);
    let block = Block::new()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(chrome);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = inner.width as usize;
    let text_width = width.saturating_sub(2);

    // Build display rows: inject urgency dividers at tier boundaries.
    // Track which display-row index corresponds to each item index.
    let mut display_items: Vec<ListItem> = vec![];
    let mut selected_display: Option<usize> = None;
    let mut prev_urgency: Option<domain::Urgency> = None;
    let mut divider_rows: Vec<usize> = vec![];

    for (item_idx, item) in items.iter().enumerate() {
        let urgency = display_item_urgency(item);
        // Inject a divider between urgency tiers, but not before the first group.
        if prev_urgency.is_some() && Some(urgency) != prev_urgency {
            divider_rows.push(display_items.len());
            display_items.push(urgency_divider(width, chrome));
        }
        prev_urgency = Some(urgency);

        if item_idx == selected {
            selected_display = Some(display_items.len());
        }

        let selected_hint = if item_idx == selected {
            match item {
                DisplayItem::Group { .. } => Some("↩ to expand".to_string()),
                DisplayItem::Single(s) => crate::display::item_hint(s),
            }
        } else {
            None
        };

        let (line_text, dim_suffix) = match item {
            DisplayItem::Group {
                label,
                items: group_items,
            } => (label.clone(), Some(format!(" ({})", group_items.len()))),
            DisplayItem::Single(_) => (display_item_line(item), None),
        };

        let dot_style = if item_idx == selected {
            Style::default()
        } else {
            urgency_style(urgency)
        };

        let item_width = text_width
            .saturating_sub(dim_suffix.as_ref().map_or(0, |s| s.chars().count()))
            .saturating_sub(selected_hint.as_ref().map_or(0, |h| h.chars().count() + 2));

        display_items.push(build_list_item(
            Span::styled("● ", dot_style),
            wrap_text(&line_text, item_width),
            dim_suffix,
            selected_hint,
        ));
    }

    let list = List::new(display_items).highlight_style(list_highlight());
    let mut ls = ListState::default();
    ls.select(selected_display);
    frame.render_stateful_widget(list, inner, &mut ls);

    // Overwrite border cells at divider rows with T-junction characters so
    // the horizontal lines visually connect to the side borders.
    let scroll = ls.offset();
    for row in divider_rows {
        if row < scroll {
            continue;
        }
        let screen_y = inner.y + (row - scroll) as u16;
        if screen_y >= inner.y + inner.height {
            break;
        }
        let buf = frame.buffer_mut();
        buf.set_string(area.x, screen_y, "├", chrome);
        buf.set_string(area.x + area.width - 1, screen_y, "┤", chrome);
    }
}

fn right_status_text(
    state: &RefreshState,
    last_updated: Option<chrono::DateTime<Utc>>,
    now: chrono::DateTime<Utc>,
) -> String {
    let age_str = |t: chrono::DateTime<Utc>| {
        let mins = (now - t).num_minutes();
        if mins == 0 {
            "just now".to_string()
        } else {
            format!("{mins}m ago")
        }
    };
    match state {
        RefreshState::InProgress => "refreshing…".to_string(),
        RefreshState::Partial(failed_sources) => {
            let time_str = last_updated
                .map(age_str)
                .unwrap_or_else(|| "unknown".to_string());
            let sources = failed_sources.join(", ");
            format!("⚠ {} unreachable (updated {time_str})", sources)
        }
        RefreshState::Failed(err) => format!("refresh failed: {err}"),
        RefreshState::Idle => last_updated
            .map(|t| format!("updated {}", age_str(t)))
            .unwrap_or_default(),
    }
}

pub(crate) fn render(frame: &mut ratatui::Frame, app: &mut App) {
    let [content_area, bar_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    match &mut app.ui.screen {
        Screen::UnifiedList {
            items,
            selected,
            filter,
        } => {
            render_unified(
                frame,
                items,
                *selected,
                filter,
                app.ui.query_input.as_deref(),
                content_area,
            );
        }
        Screen::Detail { parent, view } => {
            detail::render_detail(frame, view, parent, content_area);
        }
    }

    let right_status =
        right_status_text(&app.data.refresh_state, app.data.last_updated, Utc::now());

    let left = status_bar_left(app);

    let right_width = Span::raw(right_status.as_str()).width() as u16;
    let [bar_left, bar_right] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(bar_area);
    frame.render_widget(Paragraph::new(left).style(dim()), bar_left);
    frame.render_widget(Paragraph::new(right_status).style(dim()), bar_right);

    if app.ui.show_help {
        let keybinds = match &app.ui.screen {
            Screen::UnifiedList { .. } => KEYBINDS_LIST,
            Screen::Detail { .. } => KEYBINDS_DETAIL,
        };
        let text = format_keybinds(keybinds);
        let lines = keybinds.len() as u16;
        let width = text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
        let popup = popup_area(frame.area(), lines, width);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(text).block(Block::new().title(" Keybinds ").borders(Borders::ALL)),
            popup,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        action_hints, position_label, render, right_status_text, status_bar_left, wrap_text,
    };
    use crate::display::{DisplayItem, Filter, ListSnapshot};
    use crate::state::{
        App, DataState, DetailView, EnterAction, InvestigateAction, RefreshState, Screen, UiState,
    };
    use chrono::Utc;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::ListState;
    use ratatui::Terminal;
    use workflows::status::StatusItem;

    // ── TestBackend helpers ───────────────────────────────────────────────────

    /// Render `app` into a `width × height` buffer and return it.
    fn draw(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
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
        terminal.draw(|frame| render(frame, app1)).unwrap();
        terminal.draw(|frame| render(frame, app2)).unwrap();
        terminal.backend().buffer().clone()
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
        // Frame 1: long right text ("⚠ source-a unreachable (updated unknown)")
        // Frame 2: empty right text (Idle + no timestamp → "")
        // If Clear is broken, frame 1's characters remain in the buffer.
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
        // Frame 1: group selected → "1/2 · Press ↩ to expand (1 items)"
        // Frame 2: single CI item selected → longer left text with ↩ URL and i hint.
        // If Clear is broken, leftover chars from frame 1 bleed into frame 2.
        let group = DisplayItem::Group {
            label: "errors".to_string(),
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
        // Isolates whether ↩ causes misalignment that survives a Clear.
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
        })
    }

    fn unified_list_app(items: Vec<DisplayItem>) -> App {
        App {
            ui: UiState {
                screen: Screen::UnifiedList {
                    items,
                    selected: 0,
                    filter: Filter::default(),
                },
                ..UiState::default()
            },
            ..App::default()
        }
    }

    fn detail_app(snapshot_items: Vec<DisplayItem>, group_index: usize, sel: usize) -> App {
        let mut ls = ListState::default();
        ls.select(Some(sel));
        App {
            ui: UiState {
                screen: Screen::Detail {
                    parent: ListSnapshot {
                        items: snapshot_items,
                        selected: 0,
                        filter: Filter::default(),
                    },
                    view: DetailView {
                        group_index,
                        list_state: ls,
                    },
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
        assert_eq!(status_bar_left(&app), "something went wrong");
    }

    #[test]
    fn status_bar_unified_list_shows_position() {
        let app = unified_list_app(vec![DisplayItem::Single(pr()), DisplayItem::Single(pr())]);
        assert!(status_bar_left(&app).starts_with("1/2"));
    }

    #[test]
    fn status_bar_empty_unified_list_shows_no_position() {
        let app = unified_list_app(vec![]);
        assert!(!status_bar_left(&app).starts_with("1/"));
    }

    #[test]
    fn position_label_unified_list_shows_index_of_n() {
        let screen = Screen::UnifiedList {
            items: vec![
                DisplayItem::Single(pr()),
                DisplayItem::Single(pr()),
                DisplayItem::Single(pr()),
            ],
            selected: 1,
            filter: Filter::default(),
        };
        assert_eq!(position_label(&screen), "2/3");
    }

    #[test]
    fn position_label_empty_unified_list_is_empty() {
        let screen = Screen::UnifiedList {
            items: vec![],
            selected: 0,
            filter: Filter::default(),
        };
        assert_eq!(position_label(&screen), "");
    }

    #[test]
    fn position_label_detail_shows_index_within_group() {
        let app = detail_app(
            vec![DisplayItem::Group {
                label: "hub".to_string(),
                items: vec![pr(), pr()],
            }],
            0,
            0,
        );
        assert_eq!(position_label(app.current_screen()), "1/2");
    }

    #[test]
    fn action_hints_open_url() {
        let enter = EnterAction::OpenUrl("https://example.com".to_string());
        let inv = InvestigateAction::None;
        assert_eq!(
            action_hints(&enter, &inv),
            " · Press ↩ to open https://example.com"
        );
    }

    #[test]
    fn action_hints_expand_group() {
        let enter = EnterAction::OpenDetail {
            group_index: 0,
            item_count: 3,
        };
        let inv = InvestigateAction::None;
        assert_eq!(action_hints(&enter, &inv), " · Press ↩ to expand (3 items)");
    }

    #[test]
    fn action_hints_investigate_ci() {
        let enter = EnterAction::None;
        let inv = InvestigateAction::LaunchCi {
            repo: "owner/repo".to_string(),
            run_url: "https://example.com".to_string(),
        };
        assert_eq!(action_hints(&enter, &inv), " · Press i to investigate");
    }

    #[cfg(feature = "private")]
    #[test]
    fn action_hints_investigate_media() {
        let enter = EnterAction::None;
        let inv = InvestigateAction::LaunchMediaBlocked {
            title: "Show — S01E01".to_string(),
            error: "Invalid video file".to_string(),
        };
        assert_eq!(action_hints(&enter, &inv), " · Press i to investigate");
    }

    #[test]
    fn action_hints_combined() {
        let enter = EnterAction::OpenUrl("https://example.com".to_string());
        let inv = InvestigateAction::LaunchCi {
            repo: "owner/repo".to_string(),
            run_url: "https://example.com".to_string(),
        };
        assert_eq!(
            action_hints(&enter, &inv),
            " · Press ↩ to open https://example.com · Press i to investigate"
        );
    }

    #[test]
    fn right_status_in_progress() {
        let now = Utc::now();
        assert_eq!(
            right_status_text(&RefreshState::InProgress, None, now),
            "refreshing…"
        );
    }

    #[test]
    fn right_status_failed_shows_error_message() {
        let now = Utc::now();
        assert_eq!(
            right_status_text(
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
        assert_eq!(right_status_text(&RefreshState::Idle, None, now), "");
    }

    #[test]
    fn right_status_idle_updated_within_a_minute() {
        let now = Utc::now();
        let last_updated = now - chrono::Duration::seconds(30);
        assert_eq!(
            right_status_text(&RefreshState::Idle, Some(last_updated), now),
            "updated just now"
        );
    }

    #[test]
    fn right_status_idle_updated_minutes_ago() {
        let now = Utc::now();
        let last_updated = now - chrono::Duration::minutes(5);
        assert_eq!(
            right_status_text(&RefreshState::Idle, Some(last_updated), now),
            "updated 5m ago"
        );
    }

    #[test]
    fn right_status_partial_no_timestamp() {
        let now = Utc::now();
        assert_eq!(
            right_status_text(&RefreshState::Partial(vec!["media".to_string()]), None, now),
            "⚠ media unreachable (updated unknown)"
        );
    }

    #[test]
    fn right_status_partial_updated_within_a_minute() {
        let now = Utc::now();
        let last_updated = now - chrono::Duration::seconds(10);
        assert_eq!(
            right_status_text(
                &RefreshState::Partial(vec!["media".to_string()]),
                Some(last_updated),
                now
            ),
            "⚠ media unreachable (updated just now)"
        );
    }

    #[test]
    fn right_status_partial_multiple_sources() {
        let now = Utc::now();
        let last_updated = now - chrono::Duration::minutes(2);
        assert_eq!(
            right_status_text(
                &RefreshState::Partial(vec!["media".to_string(), "linear issues".to_string()]),
                Some(last_updated),
                now
            ),
            "⚠ media, linear issues unreachable (updated 2m ago)"
        );
    }

    #[test]
    fn wrap_text_treats_zero_width_as_one_column() {
        assert_eq!(wrap_text("abc", 0), vec!["a", "b", "c"]);
    }

    #[test]
    fn wrap_text_hard_wraps_without_dropping_characters() {
        assert_eq!(wrap_text("abcdef", 3), vec!["abc", "def"]);
    }

    #[test]
    fn wrap_text_prefers_word_boundaries() {
        assert_eq!(
            wrap_text("alpha beta gamma", 10),
            vec!["alpha beta", "gamma"]
        );
    }
}
