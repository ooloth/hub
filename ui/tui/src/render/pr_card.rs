use domain::{PullRequest, ReviewDecision};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::display::{format_age_short, merge_blocker_word};

use super::{dim, urgency_color, wrap_text};

/// Card for one PR in the split view's left list.
///
/// Layout (each item is one terminal row):
///   bullet + status + age
///   title (indented; wraps across rows when wider than `content_width`)
///   repo + #number (indented, dim)
///
/// `content_width` is the pane's interior width — the width available
/// to a row inside the bordered block. Title wrapping uses this minus
/// the 4-space indent.
///
/// Pure — no I/O, no terminal access. Style decisions live here;
/// inter-card layout (selection highlight, divider between cards) is
/// the caller's job.
pub(crate) fn pr_card_lines(pr: &PullRequest, content_width: usize) -> Vec<Line<'static>> {
    let status = pr_status_text(pr);
    let age = format_age_short(pr.age);
    let line_status = Line::from(vec![
        Span::raw(" "),
        Span::styled("●", Style::default().fg(urgency_color(pr.urgency))),
        Span::raw(format!("  {status}")),
        Span::styled(format!(" · {age}"), dim()),
    ]);

    let title_width = content_width.saturating_sub(4).max(1);
    let title_lines: Vec<Line<'static>> = wrap_text(&pr.title, title_width)
        .into_iter()
        .map(|piece| {
            Line::from(Span::styled(
                format!("    {piece}"),
                Style::default().add_modifier(Modifier::BOLD),
            ))
        })
        .collect();

    let line_repo = Line::from(Span::styled(
        format!("    {} · #{}", pr.repo, pr.number),
        dim(),
    ));

    let mut lines = Vec::with_capacity(2 + title_lines.len());
    lines.push(line_status);
    lines.extend(title_lines);
    lines.push(line_repo);
    lines
}

/// The most informative single-word status for a PR.
///
/// A merge blocker wins over review state because it's the more
/// pressing signal — the brainstorm cards lead with "conflict" even
/// when the PR also has no reviews yet.
fn pr_status_text(pr: &PullRequest) -> String {
    if let Some(blocker) = pr.merge_blocker {
        return merge_blocker_word(blocker).to_string();
    }
    match pr.review_decision {
        Some(ReviewDecision::Approved) => match pr.approval_count {
            1 => "1 approval".to_string(),
            n => format!("{n} approvals"),
        },
        Some(ReviewDecision::ChangesRequested) => "changes requested".to_string(),
        None => "no reviews".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{pr_card_lines, pr_status_text};
    use chrono::Duration;
    use domain::{MergeBlocker, PrKind, PullRequest, RepoSlug, ReviewDecision, Urgency};
    use rstest::rstest;

    /// Wide enough that the title in `pr()` fits on one row.
    const WIDE: usize = 80;

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
            head_branch: "fix/stderr".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: None,
        }
    }

    fn line_text(line: &ratatui::text::Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[rstest]
    #[case::no_reviews(None, 0, None, "no reviews")]
    #[case::changes_requested(Some(ReviewDecision::ChangesRequested), 0, None, "changes requested")]
    #[case::one_approval(Some(ReviewDecision::Approved), 1, None, "1 approval")]
    #[case::three_approvals(Some(ReviewDecision::Approved), 3, None, "3 approvals")]
    #[case::blocker_overrides_no_reviews(None, 0, Some(MergeBlocker::Conflict), "conflict")]
    #[case::blocker_overrides_approval(
        Some(ReviewDecision::Approved),
        1,
        Some(MergeBlocker::Behind),
        "behind"
    )]
    #[case::blocker_blocked(None, 0, Some(MergeBlocker::Blocked), "blocked")]
    fn status_text_picks_most_pressing_signal(
        #[case] decision: Option<ReviewDecision>,
        #[case] approvals: u32,
        #[case] blocker: Option<MergeBlocker>,
        #[case] expected: &str,
    ) {
        let mut p = pr();
        p.review_decision = decision;
        p.approval_count = approvals;
        p.merge_blocker = blocker;
        assert_eq!(pr_status_text(&p), expected);
    }

    #[test]
    fn card_status_line_has_bullet_status_and_age() {
        let lines = pr_card_lines(&pr(), WIDE);
        assert_eq!(line_text(&lines[0]), " ●  no reviews · 8d");
    }

    #[test]
    fn card_repo_line_is_indented_repo_and_number() {
        let lines = pr_card_lines(&pr(), WIDE);
        let last = lines.last().unwrap();
        assert_eq!(line_text(last), "    ooloth/hub · #159");
    }

    #[test]
    fn card_title_fits_on_one_line_at_wide_widths() {
        let lines = pr_card_lines(&pr(), WIDE);
        // 3 lines = status + 1 title + repo
        assert_eq!(lines.len(), 3);
        assert_eq!(
            line_text(&lines[1]),
            "    workflows/implement: filter claude stderr"
        );
    }

    #[test]
    fn card_title_wraps_when_narrower_than_title() {
        // Width 20 leaves 16 cols for title text after the 4-space indent.
        // "workflows/implement: filter claude stderr" must split across
        // multiple title rows; the status and repo rows are unaffected.
        let lines = pr_card_lines(&pr(), 20);
        assert!(
            lines.len() > 3,
            "expected wrapped title to produce more than 3 lines, got {}",
            lines.len()
        );
        // Status row unchanged
        assert_eq!(line_text(&lines[0]), " ●  no reviews · 8d");
        // Every middle line is the indented title (no piece exceeds 16 chars).
        for line in &lines[1..lines.len() - 1] {
            let text = line_text(line);
            assert!(
                text.starts_with("    "),
                "title row missing indent: {text:?}"
            );
            assert!(
                text.len() <= 4 + 16,
                "title row {text:?} exceeds indented width budget"
            );
        }
        // Last line is still the repo row
        assert_eq!(line_text(lines.last().unwrap()), "    ooloth/hub · #159");
    }

    #[rstest]
    #[case::seconds(Duration::seconds(30), "now")]
    #[case::minutes(Duration::minutes(45), "45m")]
    #[case::hours(Duration::hours(3), "3h")]
    #[case::days(Duration::days(8), "8d")]
    fn card_status_line_age_format(#[case] age: Duration, #[case] expected_age: &str) {
        let mut p = pr();
        p.age = age;
        let lines = pr_card_lines(&p, WIDE);
        assert!(
            line_text(&lines[0]).ends_with(expected_age),
            "expected status line to end with {expected_age:?}"
        );
    }

    #[test]
    fn card_with_conflict_shows_conflict_status() {
        let mut p = pr();
        p.merge_blocker = Some(MergeBlocker::Conflict);
        let lines = pr_card_lines(&p, WIDE);
        assert!(line_text(&lines[0]).contains("conflict"));
    }
}
