use domain::{CiStatus, PullRequest};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::display::format_age_short;

use super::{dim, pr_card::pr_status_text, urgency_color, wrap_text};

/// Right-pane detail view for `Screen::PrSplit`.
///
/// Renders directly into `area` with no block — the caller is
/// responsible for providing a padded area inside the shared outer border.
///
/// Layout (top to bottom): PR identifier, blank, bold title (wrapped),
/// structured fields, separator, Description section, Files changed section.
///
/// When the rendered content exceeds the visible pane height, the tail
/// is clipped and replaced with a dim "+N more lines" hint pointing at
/// vim-scroll (tracked in #240).
pub(super) fn render_pr_split_detail(frame: &mut ratatui::Frame, pr: &PullRequest, area: Rect) {
    let content_width = area.width as usize;
    let visible_height = area.height as usize;
    let lines = build_detail_lines(pr, content_width, visible_height);
    frame.render_widget(Paragraph::new(lines), area);
}

fn build_detail_lines(pr: &PullRequest, width: usize, visible_height: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // PR identifier (replaces the removed block title)
    lines.push(Line::from(Span::styled(
        format!("PR #{} · {}", pr.number, pr.repo),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Title (bold, wrapped)
    for piece in wrap_text(&pr.title, width.max(1)) {
        lines.push(Line::from(Span::styled(
            piece,
            Style::default().add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(""));

    // Structured fields
    lines.extend(field_lines(pr));
    lines.push(Line::from(""));

    // Files changed (sits with the metadata above the divider; Description
    // is the long-form content below.)
    lines.push(Line::from(Span::styled(
        "Files changed",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.extend(file_lines(&pr.changed_files, width));
    lines.push(Line::from(""));

    // Separator
    lines.push(separator_line(width));
    lines.push(Line::from(""));

    // Description
    lines.push(Line::from(Span::styled(
        "Description",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.extend(description_lines(pr.body.as_deref(), width));

    apply_overflow_hint(lines, visible_height)
}

fn field_lines(pr: &PullRequest) -> Vec<Line<'static>> {
    let status = pr_status_text(pr);
    let diff = format!(
        "+{} −{} in {} files",
        pr.changed_files.iter().map(|f| f.additions).sum::<u32>(),
        pr.changed_files.iter().map(|f| f.deletions).sum::<u32>(),
        pr.total_changed_files,
    );
    let checks = check_status_text(pr.ci_status);
    let updated = format!("{} ago", format_age_short(pr.age));
    vec![
        field_row("status", &status, urgency_color(pr.urgency)),
        field_row_plain("checks", &checks),
        field_row_plain("diff", &diff),
        field_row_plain("branch", &pr.head_branch),
        field_row_plain("author", &format!("@{}", pr.author)),
        field_row_plain("updated", &updated),
    ]
}

/// Field row whose value gets a colored bullet (used for "status").
fn field_row(key: &str, value: &str, value_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {key:<8}"), dim()),
        Span::styled("●  ", Style::default().fg(value_color)),
        Span::raw(value.to_string()),
    ])
}

/// Field row with no marker — just dim key, raw value.
fn field_row_plain(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {key:<8}"), dim()),
        Span::raw(format!("   {value}")),
    ])
}

fn check_status_text(ci: Option<CiStatus>) -> String {
    match ci {
        Some(CiStatus::Success) => "✓ passing".to_string(),
        Some(CiStatus::Failure) => "✗ failing".to_string(),
        Some(CiStatus::Pending) => "⏳ pending".to_string(),
        Some(CiStatus::Neutral) => "○ neutral".to_string(),
        None => "(none)".to_string(),
    }
}

fn separator_line(width: usize) -> Line<'static> {
    let dashes = "─".repeat(width.max(1));
    Line::from(Span::styled(dashes, dim()))
}

fn description_lines(body: Option<&str>, width: usize) -> Vec<Line<'static>> {
    let raw = body.filter(|s| !s.is_empty());
    match raw {
        None => vec![Line::from(Span::styled("  (no description)", dim()))],
        Some(text) => {
            let body_width = width.saturating_sub(2).max(1);
            text.lines()
                .flat_map(|paragraph| {
                    if paragraph.is_empty() {
                        vec![Line::from("")]
                    } else {
                        wrap_text(paragraph, body_width)
                            .into_iter()
                            .map(|piece| Line::from(format!("  {piece}")))
                            .collect()
                    }
                })
                .collect()
        }
    }
}

fn file_lines(files: &[domain::ChangedFile], width: usize) -> Vec<Line<'static>> {
    if files.is_empty() {
        return vec![Line::from(Span::styled("  (no files)", dim()))];
    }
    // Right-justify the diff totals when there's room.
    let max_path_len = files
        .iter()
        .map(|f| f.path.chars().count())
        .max()
        .unwrap_or(0);
    let path_col = max_path_len.min(width.saturating_sub(20).max(20));
    files
        .iter()
        .map(|f| {
            let totals = format!("+{} −{}", f.additions, f.deletions);
            let path = truncate(&f.path, path_col);
            Line::from(vec![
                Span::raw(format!("  {path:<path_col$}  ")),
                Span::styled(totals, dim()),
            ])
        })
        .collect()
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else if max <= 1 {
        s.chars().take(max).collect()
    } else {
        let mut t: String = chars[..max - 1].iter().collect();
        t.push('…');
        t
    }
}

/// If the lines exceed `visible_height`, truncate and append a dim hint.
fn apply_overflow_hint(mut lines: Vec<Line<'static>>, visible_height: usize) -> Vec<Line<'static>> {
    if lines.len() <= visible_height || visible_height == 0 {
        return lines;
    }
    let visible = visible_height.saturating_sub(1).max(1);
    let truncated = lines.len() - visible;
    lines.truncate(visible);
    lines.push(Line::from(Span::styled(
        format!("  +{truncated} more lines · vim-scroll coming in v2 (#240)"),
        dim(),
    )));
    lines
}

#[cfg(test)]
mod tests {
    use super::{
        apply_overflow_hint, check_status_text, description_lines, field_lines, file_lines,
        truncate,
    };
    use chrono::Duration;
    use domain::{
        ChangedFile, CiStatus, MergeBlocker, PrKind, PullRequest, RepoSlug, ReviewDecision, Urgency,
    };
    use ratatui::text::Line;
    use rstest::rstest;

    fn line_text(line: &Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn pr() -> PullRequest {
        PullRequest {
            number: 159,
            title: "workflows/implement: filter claude stderr".to_string(),
            repo: RepoSlug::new("ooloth", "hub"),
            url: "https://github.com/ooloth/hub/pull/159".to_string(),
            age: Duration::days(8),
            urgency: Urgency::Medium,
            kind: PrKind::Mine,
            author: "ooloth".to_string(),
            review_decision: None,
            approval_count: 0,
            comment_count: 0,
            head_branch: "fix/stderr-filter".to_string(),
            base_branch: "main".to_string(),
            body: Some("Short body.".to_string()),
            ci_status: Some(CiStatus::Success),
            changed_files: vec![
                ChangedFile {
                    path: "workflows/src/agent.rs".to_string(),
                    additions: 24,
                    deletions: 3,
                    patch: None,
                },
                ChangedFile {
                    path: "tests/agent_test.rs".to_string(),
                    additions: 18,
                    deletions: 0,
                    patch: None,
                },
            ],
            total_changed_files: 2,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: None,
        }
    }

    #[rstest]
    #[case::success(Some(CiStatus::Success), "✓ passing")]
    #[case::failure(Some(CiStatus::Failure), "✗ failing")]
    #[case::pending(Some(CiStatus::Pending), "⏳ pending")]
    #[case::neutral(Some(CiStatus::Neutral), "○ neutral")]
    #[case::none(None, "(none)")]
    fn check_status_text_covers_every_variant(
        #[case] ci: Option<CiStatus>,
        #[case] expected: &str,
    ) {
        assert_eq!(check_status_text(ci), expected);
    }

    #[test]
    fn field_lines_include_status_checks_diff_branch_author_updated() {
        let lines = field_lines(&pr());
        assert_eq!(lines.len(), 6);
        assert!(line_text(&lines[0]).contains("status"));
        assert!(line_text(&lines[1]).contains("checks"));
        assert!(line_text(&lines[2]).contains("diff"));
        assert!(line_text(&lines[3]).contains("branch"));
        assert!(line_text(&lines[4]).contains("author"));
        assert!(line_text(&lines[5]).contains("updated"));
    }

    #[test]
    fn field_lines_status_uses_pr_status_text_priority() {
        // Blocker beats review state.
        let mut p = pr();
        p.review_decision = Some(ReviewDecision::Approved);
        p.merge_blocker = Some(MergeBlocker::Conflict);
        let lines = field_lines(&p);
        assert!(line_text(&lines[0]).ends_with("conflict"));
    }

    #[test]
    fn field_lines_diff_sums_additions_and_deletions() {
        let lines = field_lines(&pr());
        assert!(line_text(&lines[2]).contains("+42 −3 in 2 files"));
    }

    #[test]
    fn description_lines_returns_placeholder_when_body_is_none() {
        let lines = description_lines(None, 60);
        assert_eq!(lines.len(), 1);
        assert!(line_text(&lines[0]).contains("(no description)"));
    }

    #[test]
    fn description_lines_returns_placeholder_when_body_is_empty() {
        let lines = description_lines(Some(""), 60);
        assert_eq!(lines.len(), 1);
        assert!(line_text(&lines[0]).contains("(no description)"));
    }

    #[test]
    fn description_lines_indents_each_line_by_two_spaces() {
        let lines = description_lines(Some("first paragraph"), 60);
        assert_eq!(line_text(&lines[0]), "  first paragraph");
    }

    #[test]
    fn description_lines_wraps_long_paragraphs() {
        let body = "a b c d e f g h i j k l m n o p q r s t";
        let lines = description_lines(Some(body), 10);
        // Indent = 2, so body wraps at 8 columns. Should produce multiple lines.
        assert!(lines.len() > 1, "expected wrap, got {} lines", lines.len());
        for line in &lines {
            assert!(line_text(line).starts_with("  "));
        }
    }

    #[test]
    fn description_lines_preserves_blank_paragraph_separators() {
        let lines = description_lines(Some("first\n\nsecond"), 60);
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert_eq!(texts, vec!["  first", "", "  second"]);
    }

    #[test]
    fn file_lines_returns_placeholder_when_no_files() {
        let lines = file_lines(&[], 80);
        assert_eq!(lines.len(), 1);
        assert!(line_text(&lines[0]).contains("(no files)"));
    }

    #[test]
    fn file_lines_renders_path_and_diff_totals() {
        let files = vec![ChangedFile {
            path: "workflows/src/agent.rs".to_string(),
            additions: 24,
            deletions: 3,
            patch: None,
        }];
        let lines = file_lines(&files, 80);
        let text = line_text(&lines[0]);
        assert!(text.contains("workflows/src/agent.rs"));
        assert!(text.contains("+24 −3"));
    }

    #[test]
    fn truncate_returns_original_when_fits() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_uses_ellipsis_when_too_long() {
        assert_eq!(truncate("hello world", 8), "hello w…");
    }

    #[test]
    fn overflow_hint_skipped_when_lines_fit() {
        let lines: Vec<Line<'static>> = (0..5).map(|i| Line::from(format!("line {i}"))).collect();
        let result = apply_overflow_hint(lines.clone(), 10);
        let texts: Vec<String> = result.iter().map(line_text).collect();
        assert_eq!(
            texts,
            vec!["line 0", "line 1", "line 2", "line 3", "line 4"]
        );
    }

    #[test]
    fn overflow_hint_truncates_and_appends_when_too_tall() {
        let lines: Vec<Line<'static>> = (0..10).map(|i| Line::from(format!("line {i}"))).collect();
        let result = apply_overflow_hint(lines, 5);
        assert_eq!(result.len(), 5);
        // Last line is the hint
        let hint = line_text(result.last().unwrap());
        assert!(hint.contains("more lines"), "got hint: {hint}");
        assert!(hint.contains("#240"));
        // Earlier lines preserved
        assert_eq!(line_text(&result[0]), "line 0");
    }

    #[test]
    fn overflow_hint_visible_height_zero_returns_input_unchanged() {
        let lines: Vec<Line<'static>> = (0..3).map(|i| Line::from(format!("line {i}"))).collect();
        let result = apply_overflow_hint(lines.clone(), 0);
        assert_eq!(result.len(), 3);
    }
}
