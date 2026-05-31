use domain::{PullRequest, ReviewDecision};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::display::merge_blocker_word;

use super::{dim, urgency_color, wrap_text};

/// Card for one PR in the split view's left list.
///
/// Layout (each item is one terminal row):
///   bullet + title (first line; wraps with 4-space indent on continuations)
///   repo · #number · @author · review info (dim metadata; approvals colored green)
///
/// `content_width` is the pane's interior width. Title wrapping uses
/// this minus the 4-char bullet prefix.
///
/// Pure — no I/O, no terminal access. Style decisions live here;
/// inter-card layout (selection highlight) is the caller's job.
pub(crate) fn pr_card_lines(pr: &PullRequest, content_width: usize) -> Vec<Line<'static>> {
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

    let (review_text, review_style) = review_info(pr);
    lines.push(Line::from(vec![
        Span::styled(format!("    {} · #{}", pr.repo, pr.number), dim()),
        Span::styled(format!(" · @{}", pr.author), dim()),
        Span::styled(format!(" · {review_text}"), review_style),
    ]));

    lines
}

fn review_info(pr: &PullRequest) -> (String, Style) {
    if pr.approval_count > 0 {
        let text = match pr.approval_count {
            1 => "1 approval".to_string(),
            n => format!("{n} approvals"),
        };
        (text, Style::default().fg(Color::Green))
    } else {
        let text = match pr.review_decision {
            Some(ReviewDecision::ChangesRequested) => "changes requested".to_string(),
            _ => "0 reviews".to_string(),
        };
        (text, dim())
    }
}

/// All text visible in a PR card, for use as filter match text.
///
/// Calls `pr_card_lines` at a very wide width (no wrapping) and joins
/// every span from every line with a space. Any change to the card
/// layout automatically updates what is searchable.
pub(crate) fn pr_card_text(pr: &PullRequest) -> String {
    pr_card_lines(pr, usize::MAX)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
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
        None => "0 reviews".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{pr_card_lines, pr_status_text, review_info};
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
    #[case::no_reviews(None, 0, None, "0 reviews")]
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
    fn card_meta_line_has_repo_number_author_and_review_info() {
        let lines = pr_card_lines(&pr(), WIDE);
        let meta = line_text(lines.last().unwrap());
        assert!(
            !meta.contains("●"),
            "meta should not have bullet, got: {meta:?}"
        );
        assert!(meta.contains("ooloth/hub · #159"), "got: {meta:?}");
        assert!(meta.contains("@ooloth"), "got: {meta:?}");
        assert!(meta.contains("0 reviews"), "got: {meta:?}");
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
        assert!(meta.contains("ooloth/hub"), "meta missing repo: {meta:?}");
        assert!(
            meta.contains("0 reviews"),
            "meta missing review info: {meta:?}"
        );
    }

    #[test]
    fn review_info_shows_approval_count_when_any_approvals() {
        let mut p = pr();
        p.review_decision = Some(ReviewDecision::Approved);
        p.approval_count = 2;
        let (text, style) = review_info(&p);
        assert_eq!(text, "2 approvals");
        assert_eq!(style.fg, Some(ratatui::style::Color::Green));
    }

    #[test]
    fn review_info_shows_changes_requested_text() {
        let mut p = pr();
        p.review_decision = Some(ReviewDecision::ChangesRequested);
        let (text, _) = review_info(&p);
        assert_eq!(text, "changes requested");
    }

    #[test]
    fn review_info_shows_no_reviews_by_default() {
        let (text, _) = review_info(&pr());
        assert_eq!(text, "0 reviews");
    }
}
