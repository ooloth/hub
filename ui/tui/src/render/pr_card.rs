use domain::{PullRequest, ReviewDecision};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::display::{format_age_short, merge_blocker_word};

use super::{dim, urgency_color};

/// Three-line card for one PR in the split view's left list.
///
/// Line 1: bullet + status + age
/// Line 2: title (indented)
/// Line 3: repo + number (indented, dim)
///
/// Pure — no I/O, no terminal access. Style decisions live here; layout
/// (selection highlight, separator between cards) is the caller's job.
pub(crate) fn pr_card_lines(pr: &PullRequest) -> [Line<'static>; 3] {
    let status = pr_status_text(pr);
    let age = format_age_short(pr.age);
    let line1 = Line::from(vec![
        Span::raw(" "),
        Span::styled("●", Style::default().fg(urgency_color(pr.urgency))),
        Span::raw(format!("  {status}")),
        Span::styled(format!(" · {age}"), dim()),
    ]);
    let line2 = Line::from(Span::styled(
        format!("    {}", pr.title),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    let line3 = Line::from(Span::styled(
        format!("    {} · #{}", pr.repo, pr.number),
        dim(),
    ));
    [line1, line2, line3]
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

    #[rstest]
    // No decision, no blocker → "no reviews"
    #[case::no_reviews(None, 0, None, "no reviews")]
    // ChangesRequested → "changes requested" (ignores approval count)
    #[case::changes_requested(Some(ReviewDecision::ChangesRequested), 0, None, "changes requested")]
    // Approved with 1 → "1 approval" (singular)
    #[case::one_approval(Some(ReviewDecision::Approved), 1, None, "1 approval")]
    // Approved with 3 → "3 approvals" (plural)
    #[case::three_approvals(Some(ReviewDecision::Approved), 3, None, "3 approvals")]
    // Any merge blocker wins over review state.
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
    fn card_lines_first_line_has_bullet_status_and_age() {
        let p = pr();
        let [line1, _, _] = pr_card_lines(&p);
        let rendered: String = line1.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rendered, " ●  no reviews · 8d");
    }

    #[test]
    fn card_lines_second_line_is_indented_title() {
        let p = pr();
        let [_, line2, _] = pr_card_lines(&p);
        let rendered: String = line2.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rendered, "    workflows/implement: filter claude stderr");
    }

    #[test]
    fn card_lines_third_line_is_indented_repo_and_number() {
        let p = pr();
        let [_, _, line3] = pr_card_lines(&p);
        let rendered: String = line3.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rendered, "    ooloth/hub · #159");
    }

    #[rstest]
    #[case::seconds(Duration::seconds(30), "now")]
    #[case::minutes(Duration::minutes(45), "45m")]
    #[case::hours(Duration::hours(3), "3h")]
    #[case::days(Duration::days(8), "8d")]
    fn card_first_line_age_format_matches_format_age_short(
        #[case] age: Duration,
        #[case] expected_age: &str,
    ) {
        let mut p = pr();
        p.age = age;
        let [line1, _, _] = pr_card_lines(&p);
        let rendered: String = line1.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            rendered.ends_with(expected_age),
            "expected line1 {rendered:?} to end with {expected_age:?}"
        );
    }

    #[test]
    fn card_lines_with_conflict_shows_conflict_status() {
        let mut p = pr();
        p.merge_blocker = Some(MergeBlocker::Conflict);
        let [line1, _, _] = pr_card_lines(&p);
        let rendered: String = line1.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(rendered.contains("conflict"), "got {rendered:?}");
    }
}
