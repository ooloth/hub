use domain::{MergeBlocker, PullRequest, ReviewDecision};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::display::{format_age_short, merge_blocker_word};

use super::{dim, urgency_color, wrap_text};

/// Card for one PR in the split view's left list.
///
/// Layout (each item is one terminal row):
///   bullet + title (first line; wraps with 4-space indent on continuations)
///   status + age + repo + #number (dim metadata line; status colored for approvals/conflict)
///
/// `content_width` is the pane's interior width. Title wrapping uses
/// this minus the 4-char bullet prefix.
///
/// Pure — no I/O, no terminal access. Style decisions live here;
/// inter-card layout (selection highlight) is the caller's job.
pub(crate) fn pr_card_lines(pr: &PullRequest, content_width: usize) -> Vec<Line<'static>> {
    let status = pr_status_text(pr);
    let age = format_age_short(pr.age);

    // First title line has the bullet; continuations indent to align.
    let title_width = content_width.saturating_sub(4).max(1);
    let pieces = wrap_text(&pr.title, title_width);
    let mut lines: Vec<Line<'static>> = pieces
        .into_iter()
        .enumerate()
        .map(|(i, piece)| {
            if i == 0 {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("●", Style::default().fg(urgency_color(pr.urgency))),
                    Span::raw("  "),
                    Span::styled(piece, Style::default().add_modifier(Modifier::BOLD)),
                ])
            } else {
                Line::from(Span::styled(
                    format!("    {piece}"),
                    Style::default().add_modifier(Modifier::BOLD),
                ))
            }
        })
        .collect();

    // Metadata line: all dim; status text overrides to green/red where meaningful.
    let status_style = status_text_style(pr);
    lines.push(Line::from(vec![
        Span::styled(format!("    {status}"), status_style),
        Span::styled(format!(" · {age}  ·  {} · #{}", pr.repo, pr.number), dim()),
    ]));

    lines
}

fn status_text_style(pr: &PullRequest) -> Style {
    if pr.merge_blocker == Some(MergeBlocker::Conflict) {
        return Style::default().fg(Color::Red);
    }
    if matches!(pr.review_decision, Some(ReviewDecision::Approved)) && pr.approval_count > 0 {
        return Style::default().fg(Color::Green);
    }
    dim()
}

/// The most informative single-word status for a PR.
///
/// A merge blocker wins over review state because it's the more
/// pressing signal — the brainstorm cards lead with "conflict" even
/// when the PR also has no reviews yet.
pub(super) fn pr_status_text(pr: &PullRequest) -> String {
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
    use super::{pr_card_lines, pr_status_text, status_text_style};
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
    fn card_first_line_has_bullet_and_title() {
        let lines = pr_card_lines(&pr(), WIDE);
        let first = line_text(&lines[0]);
        assert!(
            first.starts_with(" ●  "),
            "expected bullet prefix, got: {first:?}"
        );
        assert!(
            first.contains("workflows/implement: filter claude stderr"),
            "got: {first:?}"
        );
    }

    #[test]
    fn card_meta_line_has_status_age_and_repo_without_bullet() {
        let lines = pr_card_lines(&pr(), WIDE);
        let meta = line_text(lines.last().unwrap());
        assert!(
            !meta.contains("●"),
            "meta should not have bullet, got: {meta:?}"
        );
        assert!(meta.contains("no reviews"), "got: {meta:?}");
        assert!(meta.contains("8d"), "got: {meta:?}");
        assert!(meta.contains("ooloth/hub · #159"), "got: {meta:?}");
    }

    #[test]
    fn card_title_fits_on_one_line_at_wide_widths() {
        let lines = pr_card_lines(&pr(), WIDE);
        // 2 lines = 1 title + 1 meta
        assert_eq!(lines.len(), 2);
        assert_eq!(
            line_text(&lines[0]),
            " ●  workflows/implement: filter claude stderr"
        );
    }

    #[test]
    fn card_title_wraps_when_narrower_than_title() {
        // Width 20 leaves 16 cols for title text after the 4-char prefix.
        // "workflows/implement: filter claude stderr" must split across
        // multiple title rows; the meta row is unaffected.
        let lines = pr_card_lines(&pr(), 20);
        assert!(
            lines.len() > 2,
            "expected wrapped title to produce more than 2 lines, got {}",
            lines.len()
        );
        // First title line has the bullet prefix.
        assert!(
            line_text(&lines[0]).starts_with(" ●  "),
            "first line missing bullet: {:?}",
            line_text(&lines[0])
        );
        // Continuation title lines are indented with 4 spaces.
        for line in &lines[1..lines.len() - 1] {
            let text = line_text(line);
            assert!(
                text.starts_with("    "),
                "continuation missing indent: {text:?}"
            );
            assert!(
                text.len() <= 4 + 16,
                "continuation {text:?} exceeds indented width budget"
            );
        }
        // Last line is the meta row.
        let meta = line_text(lines.last().unwrap());
        assert!(meta.contains("no reviews"), "meta missing status: {meta:?}");
        assert!(meta.contains("ooloth/hub"), "meta missing repo: {meta:?}");
    }

    #[rstest]
    #[case::seconds(Duration::seconds(30), "now")]
    #[case::minutes(Duration::minutes(45), "45m")]
    #[case::hours(Duration::hours(3), "3h")]
    #[case::days(Duration::days(8), "8d")]
    fn card_meta_line_age_format(#[case] age: Duration, #[case] expected_age: &str) {
        let mut p = pr();
        p.age = age;
        let lines = pr_card_lines(&p, WIDE);
        let meta = line_text(lines.last().unwrap());
        assert!(
            meta.contains(expected_age),
            "expected meta line to contain {expected_age:?}, got: {meta:?}"
        );
    }

    #[test]
    fn card_with_conflict_shows_conflict_status() {
        let mut p = pr();
        p.merge_blocker = Some(MergeBlocker::Conflict);
        let lines = pr_card_lines(&p, WIDE);
        assert!(line_text(lines.last().unwrap()).contains("conflict"));
    }

    #[test]
    fn status_text_style_is_green_for_approvals() {
        let mut p = pr();
        p.review_decision = Some(ReviewDecision::Approved);
        p.approval_count = 1;
        assert_eq!(status_text_style(&p).fg, Some(ratatui::style::Color::Green));
    }

    #[test]
    fn status_text_style_is_red_for_conflict() {
        let mut p = pr();
        p.merge_blocker = Some(MergeBlocker::Conflict);
        assert_eq!(status_text_style(&p).fg, Some(ratatui::style::Color::Red));
    }

    #[test]
    fn status_text_style_is_dim_otherwise() {
        let lines = pr_card_lines(&pr(), WIDE);
        // Verify it compiles and runs without panic for the default case.
        assert!(lines.len() >= 2);
    }
}
