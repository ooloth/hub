use domain::pr_activity::Opinion;
use domain::{CiStatus, PullRequest, ReviewStatus};
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
        review_status_spans(pr.review_status()),
    ));

    let approvals = pr.activity.count(Opinion::Approved);
    let changes_requested = pr.activity.count(Opinion::ChangesRequested);
    let total_reviews = approvals + changes_requested;
    if total_reviews > 0 {
        let label = if changes_requested == 0 {
            if approvals == 1 {
                "1 approval".to_string()
            } else {
                format!("{approvals} approvals")
            }
        } else if total_reviews == 1 {
            "1 review".to_string()
        } else {
            format!("{total_reviews} reviews")
        };
        lines.push(field_row_plain("reviews", &label));
    }

    let comment_label = match pr.comment_count {
        1 => "1 comment".to_string(),
        n => format!("{n} comments"),
    };
    lines.push(field_row_plain("comments", &comment_label));

    if let Some(blocker) = pr.merge_readiness.blocker() {
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

fn review_status_spans(status: ReviewStatus) -> Vec<Span<'static>> {
    match status {
        ReviewStatus::Approved(_) => {
            vec![Span::styled("Approved", Style::default().fg(Color::Green))]
        }
        ReviewStatus::ChangesRequested => {
            vec![Span::styled(
                "Changes requested",
                Style::default().fg(Color::Red),
            )]
        }
        ReviewStatus::NoReviews => vec![Span::styled("0 reviews", dim())],
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
        let mut t: String = chars.get(..max - 1).unwrap_or_default().iter().collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::{diff_spans, dim, file_lines, pr_right_column_lines, truncate};
    use chrono::Duration;
    use domain::{
        ChangedFile, CiStatus, MergeBlocker, MergeReadiness, PrKind, PullRequest, ReviewDecision,
        Urgency,
    };
    use ratatui::{style::Color, text::Line};
    use rstest::rstest;

    fn line_text(line: &Line) -> String {
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
            url: "https://github.com/ooloth/hub/pull/42".to_string(),
            age: Duration::days(3),
            urgency: Urgency::High,
            kind: PrKind::ToReview,
            author: "alice".to_string(),
            review_decision: Some(ReviewDecision::Approved),
            comment_count: 5,
            head_branch: "feat/two-col".to_string(),
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
            merge_readiness: MergeReadiness::Blocked(MergeBlocker::Conflict),
            activity: crate::test_support::verdicts(2, 0),
            ..crate::test_support::pull_request()
        }
    }

    const W: usize = 40;
    const H: usize = 40;

    /// The right column for `pr` at height `height`, one line of text per row.
    fn column_text(pr: &PullRequest, height: usize) -> String {
        pr_right_column_lines(pr, W, height, dim())
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn pr_right_column_lines_snapshot_fully_populated_pr() {
        insta::assert_snapshot!(column_text(&pr_with_all_fields(), H));
    }

    #[test]
    fn the_reviews_row_is_left_out_when_nobody_has_approved() {
        let pr = PullRequest {
            activity: crate::test_support::verdicts(0, 0),
            review_decision: None,
            ..pr_with_all_fields()
        };
        let lines = pr_right_column_lines(&pr, W, H, dim());
        let has_reviews_row = lines.iter().any(|l| line_text(l).contains("reviews "));
        assert!(!has_reviews_row, "expected no reviews row");
    }

    #[rstest]
    #[case::one(1, "1 approval")]
    #[case::many(3, "3 approvals")]
    fn the_reviews_row_counts_the_approvals(#[case] count: u32, #[case] expected: &str) {
        let pr = PullRequest {
            activity: crate::test_support::verdicts(count, 0),
            ..pr_with_all_fields()
        };
        let all_text = column_text(&pr, H);
        assert!(
            all_text.contains(expected),
            "expected '{expected}' in:\n{all_text}"
        );
    }

    #[test]
    fn the_reviews_row_sums_approvals_and_changes_requested() {
        let pr = PullRequest {
            activity: crate::test_support::verdicts(1, 1),
            ..pr_with_all_fields()
        };
        let all_text = column_text(&pr, H);
        assert!(
            all_text.contains("2 reviews"),
            "expected '2 reviews' in:\n{all_text}"
        );
        assert!(
            !all_text.contains("1 approval"),
            "did not expect the approval-only wording once changes-requested reviews exist:\n{all_text}"
        );
    }

    #[test]
    fn the_status_row_shows_changes_requested_alongside_approvals() {
        let pr = PullRequest {
            activity: crate::test_support::verdicts(1, 1),
            ..pr_with_all_fields()
        };
        let all_text = column_text(&pr, H);
        assert!(
            all_text.contains("Changes requested"),
            "expected the status row to say 'Changes requested':\n{all_text}"
        );
    }

    #[test]
    fn the_blocker_row_is_left_out_when_nothing_blocks_the_merge() {
        let pr = PullRequest {
            merge_readiness: MergeReadiness::Ready,
            ..pr_with_all_fields()
        };
        let all_text = column_text(&pr, H);
        assert!(
            !all_text.contains("blocker"),
            "expected no blocker row, got:\n{all_text}"
        );
    }

    #[rstest]
    #[case::conflict(MergeBlocker::Conflict, "conflict")]
    #[case::behind(MergeBlocker::Behind, "behind")]
    #[case::blocked(MergeBlocker::Blocked, "blocked")]
    fn the_blocker_row_names_what_blocks_the_merge(
        #[case] blocker: MergeBlocker,
        #[case] expected_word: &str,
    ) {
        let pr = PullRequest {
            merge_readiness: MergeReadiness::Blocked(blocker),
            ..pr_with_all_fields()
        };
        let all_text = column_text(&pr, H);
        assert!(
            all_text.contains(expected_word),
            "expected '{expected_word}' in:\n{all_text}"
        );
    }

    #[test]
    fn the_file_section_is_separated_when_space_allows() {
        let lines = pr_right_column_lines(&pr_with_all_fields(), W, H, dim());
        let has_separator = lines.iter().any(|l| line_text(l).contains('─'));
        assert!(
            has_separator,
            "expected separator line indicating file section"
        );
    }

    #[test]
    fn the_file_list_is_left_out_when_there_is_no_space() {
        let all_text = column_text(&pr_with_all_fields(), 5);
        assert!(
            !all_text.contains("mod.rs"),
            "expected no file list when height is tiny:\n{all_text}"
        );
    }

    #[test]
    fn a_file_list_too_long_for_the_space_ends_with_a_more_hint() {
        let pr = PullRequest {
            changed_files: (0..10)
                .map(|i| ChangedFile {
                    path: format!("file{i}.rs"),
                    additions: 1,
                    deletions: 0,
                    patch: None,
                })
                .collect(),
            total_changed_files: 10,
            ..pr_with_all_fields()
        };
        let field_count = 12;
        let height = field_count + 2 + 3;
        let all_text = column_text(&pr, height);
        assert!(
            all_text.contains("more…"),
            "expected overflow hint:\n{all_text}"
        );
    }
}
