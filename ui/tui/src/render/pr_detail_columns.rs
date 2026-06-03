use domain::{CiStatus, PullRequest, ReviewDecision};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::display::{format_age_short, merge_blocker_word};

use super::dim;

/// Metadata lines for the right column of the two-column PR detail pane.
///
/// Returned in display order: checks, status, reviews (if any), comments,
/// blocker (if any), diff, author, updated, repo, branch — then a separator
/// and the per-file list. Files that don't fit within `available_height` are
/// clipped with a "+N more…" hint.
pub(super) fn pr_right_column_lines(
    pr: &PullRequest,
    width: usize,
    available_height: usize,
    separator_style: Style,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(field_row_spans("checks", check_status_spans(pr.ci_status)));
    lines.push(field_row_spans(
        "status",
        review_decision_spans(pr.review_decision),
    ));

    if pr.approval_count > 0 {
        let label = if pr.approval_count == 1 {
            "1 approval".to_string()
        } else {
            format!("{} approvals", pr.approval_count)
        };
        lines.push(field_row_plain("reviews", &label));
    }

    let comment_label = match pr.comment_count {
        1 => "1 comment".to_string(),
        n => format!("{n} comments"),
    };
    lines.push(field_row_plain("comments", &comment_label));

    if let Some(blocker) = pr.merge_blocker {
        lines.push(field_row_spans(
            "blocker",
            vec![Span::styled(
                merge_blocker_word(blocker),
                Style::default().fg(Color::Red),
            )],
        ));
    }

    let additions: u32 = pr.changed_files.iter().map(|f| f.additions).sum();
    let deletions: u32 = pr.changed_files.iter().map(|f| f.deletions).sum();
    let mut diff = diff_spans(additions, deletions);
    diff.push(Span::raw(format!(" in {} files", pr.total_changed_files)));
    lines.push(field_row_spans("diff", diff));

    lines.push(field_row_plain("author", &format!("@{}", pr.author)));
    lines.push(field_row_plain(
        "updated",
        &format!("{} ago", format_age_short(pr.age)),
    ));
    lines.push(field_row_plain("repo", &pr.repo.to_string()));
    lines.push(field_row_plain("branch", &pr.head_branch));

    // Separator + per-file list, clipped to available space.
    let separator_overhead = 2; // blank line + separator line
    let used = lines.len() + separator_overhead;
    let space_for_files = available_height.saturating_sub(used);
    if space_for_files > 0 && !pr.changed_files.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─".repeat(width.max(1)),
            separator_style,
        )));
        let all_files = file_lines(&pr.changed_files, width);
        let total = all_files.len();
        if total <= space_for_files {
            lines.extend(all_files);
        } else {
            let visible = space_for_files.saturating_sub(1).max(1);
            lines.extend(all_files.into_iter().take(visible));
            let hidden = total - visible;
            lines.push(Line::from(Span::styled(format!("+{hidden} more…"), dim())));
        }
    }

    lines
}

fn field_row_plain(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<8}"), dim()),
        Span::raw(format!("   {value}")),
    ])
}

fn field_row_spans(key: &str, value_spans: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::styled(format!("{key:<8}"), dim()), Span::raw("   ")];
    spans.extend(value_spans);
    Line::from(spans)
}

fn diff_spans(additions: u32, deletions: u32) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!("+{additions}"), Style::default().fg(Color::Green)),
        Span::raw(" "),
        Span::styled(format!("−{deletions}"), Style::default().fg(Color::Red)),
    ]
}

fn check_status_spans(ci: Option<CiStatus>) -> Vec<Span<'static>> {
    match ci {
        Some(CiStatus::Success) => {
            vec![Span::styled("✓ passing", Style::default().fg(Color::Green))]
        }
        Some(CiStatus::Failure) => vec![Span::styled("✗ failing", Style::default().fg(Color::Red))],
        Some(CiStatus::Pending) => vec![Span::styled(
            "⏳ pending",
            Style::default().fg(Color::Yellow),
        )],
        Some(CiStatus::Neutral) => vec![Span::styled("○ neutral", dim())],
        None => vec![Span::styled("(none)", dim())],
    }
}

fn review_decision_spans(decision: Option<ReviewDecision>) -> Vec<Span<'static>> {
    match decision {
        Some(ReviewDecision::Approved) => {
            vec![Span::styled("Approved", Style::default().fg(Color::Green))]
        }
        Some(ReviewDecision::ChangesRequested) => {
            vec![Span::styled(
                "Changes requested",
                Style::default().fg(Color::Red),
            )]
        }
        None => vec![Span::styled("0 reviews", dim())],
    }
}

fn file_lines(files: &[domain::ChangedFile], width: usize) -> Vec<Line<'static>> {
    if files.is_empty() {
        return vec![Line::from(Span::styled("(no files)", dim()))];
    }
    let max_path_len = files
        .iter()
        .map(|f| f.path.chars().count())
        .max()
        .unwrap_or(0);
    let path_col = max_path_len.min(width.saturating_sub(20).max(20));
    files
        .iter()
        .map(|f| {
            let path = truncate(&f.path, path_col);
            let mut spans = vec![Span::raw(format!("{path:<path_col$}  "))];
            spans.extend(diff_spans(f.additions, f.deletions));
            Line::from(spans)
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

#[cfg(test)]
mod tests {
    use super::{diff_spans, dim, file_lines, pr_right_column_lines, truncate};
    use chrono::Duration;
    use domain::{
        ChangedFile, CiStatus, MergeBlocker, PrKind, PullRequest, RepoSlug, ReviewDecision, Urgency,
    };
    use ratatui::{style::Color, text::Line};
    use rstest::rstest;

    fn line_text(line: &Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn diff_spans_additions_are_green_and_deletions_are_red() {
        let spans = diff_spans(10, 5);
        assert_eq!(spans[0].content, "+10");
        assert_eq!(spans[0].style.fg, Some(Color::Green));
        assert_eq!(spans[2].content, "−5");
        assert_eq!(spans[2].style.fg, Some(Color::Red));
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

    // --- pr_right_column_lines ---

    fn pr_with_all_fields() -> PullRequest {
        PullRequest {
            number: 42,
            title: "feat: add two-column PR detail".to_string(),
            repo: RepoSlug::new("ooloth", "hub"),
            url: "https://github.com/ooloth/hub/pull/42".to_string(),
            age: Duration::days(3),
            urgency: Urgency::High,
            kind: PrKind::ToReview,
            author: "alice".to_string(),
            review_decision: Some(ReviewDecision::Approved),
            approval_count: 2,
            comment_count: 5,
            head_branch: "feat/two-col".to_string(),
            base_branch: "main".to_string(),
            body: Some("Some description.".to_string()),
            ci_status: Some(CiStatus::Success),
            changed_files: vec![
                ChangedFile {
                    path: "ui/tui/src/render/mod.rs".to_string(),
                    additions: 30,
                    deletions: 10,
                    patch: None,
                },
                ChangedFile {
                    path: "ui/tui/src/render/pr_detail_columns.rs".to_string(),
                    additions: 80,
                    deletions: 5,
                    patch: None,
                },
            ],
            total_changed_files: 2,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: Some(MergeBlocker::Conflict),
        }
    }

    const W: usize = 40;
    const H: usize = 40;

    #[test]
    fn pr_right_column_lines_snapshot_fully_populated_pr() {
        let lines = pr_right_column_lines(&pr_with_all_fields(), W, H, dim());
        let text: Vec<String> = lines.iter().map(line_text).collect();
        insta::assert_snapshot!(text.join("\n"));
    }

    #[test]
    fn pr_right_column_lines_omits_reviews_row_when_approval_count_is_zero() {
        let mut p = pr_with_all_fields();
        p.approval_count = 0;
        p.review_decision = None;
        let lines = pr_right_column_lines(&p, W, H, dim());
        let has_reviews_row = lines.iter().any(|l| line_text(l).contains("reviews "));
        assert!(!has_reviews_row, "expected no reviews row");
    }

    #[rstest]
    #[case::one(1, "1 approval")]
    #[case::many(3, "3 approvals")]
    fn pr_right_column_lines_shows_reviews_row_when_approval_count_nonzero(
        #[case] count: u32,
        #[case] expected: &str,
    ) {
        let mut p = pr_with_all_fields();
        p.approval_count = count;
        let lines = pr_right_column_lines(&p, W, H, dim());
        let all_text: String = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(
            all_text.contains(expected),
            "expected '{expected}' in:\n{all_text}"
        );
    }

    #[test]
    fn pr_right_column_lines_omits_blocker_row_when_merge_blocker_is_none() {
        let mut p = pr_with_all_fields();
        p.merge_blocker = None;
        let lines = pr_right_column_lines(&p, W, H, dim());
        let all_text: String = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(
            !all_text.contains("blocker"),
            "expected no blocker row, got:\n{all_text}"
        );
    }

    #[rstest]
    #[case::conflict(MergeBlocker::Conflict, "conflict")]
    #[case::behind(MergeBlocker::Behind, "behind")]
    #[case::blocked(MergeBlocker::Blocked, "blocked")]
    fn pr_right_column_lines_shows_blocker_row_with_correct_word(
        #[case] blocker: MergeBlocker,
        #[case] expected_word: &str,
    ) {
        let mut p = pr_with_all_fields();
        p.merge_blocker = Some(blocker);
        let lines = pr_right_column_lines(&p, W, H, dim());
        let all_text: String = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(
            all_text.contains(expected_word),
            "expected '{expected_word}' in:\n{all_text}"
        );
    }

    #[test]
    fn pr_right_column_lines_shows_file_section_separator_when_space_allows() {
        let lines = pr_right_column_lines(&pr_with_all_fields(), W, H, dim());
        let has_separator = lines.iter().any(|l| line_text(l).contains('─'));
        assert!(
            has_separator,
            "expected separator line indicating file section"
        );
    }

    #[test]
    fn pr_right_column_lines_omits_file_list_when_no_space() {
        let lines = pr_right_column_lines(&pr_with_all_fields(), W, 5, dim());
        let all_text: String = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(
            !all_text.contains("mod.rs"),
            "expected no file list when height is tiny:\n{all_text}"
        );
    }

    #[test]
    fn pr_right_column_lines_clips_file_list_with_more_hint_when_overflow() {
        let mut p = pr_with_all_fields();
        p.changed_files = (0..10)
            .map(|i| ChangedFile {
                path: format!("file{i}.rs"),
                additions: 1,
                deletions: 0,
                patch: None,
            })
            .collect();
        p.total_changed_files = 10;
        let field_count = 12;
        let height = field_count + 2 + 3;
        let lines = pr_right_column_lines(&p, W, height, dim());
        let all_text: String = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(
            all_text.contains("more…"),
            "expected overflow hint:\n{all_text}"
        );
    }
}
