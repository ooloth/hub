//! Which pull requests a refresh found waiting on me.
use domain::turns_seen::{TurnCoverage, TurnsSeen};

use crate::status::{Refresh, StatusItem, MY_OPEN_PRS, PRS_AWAITING_REVIEW};

/// The pull requests `refresh` found waiting on me.
///
/// This is the one place a signal becomes something to notify about. Only pull requests are
/// today; another kind of signal would be selected here too.
#[must_use]
pub fn turns_seen(refresh: &Refresh) -> TurnsSeen {
    let prs = refresh.report.items.iter().filter_map(|item| match item {
        StatusItem::Pr(pr) => Some(pr),
        _ => None,
    });
    let answered = refresh.sources.answered();
    let heard_from = |source: &str| answered.iter().any(|name| name == source);
    let coverage = if heard_from(MY_OPEN_PRS) && heard_from(PRS_AWAITING_REVIEW) {
        TurnCoverage::Complete
    } else {
        TurnCoverage::Incomplete
    };
    TurnsSeen::from_prs(prs, coverage)
}

#[cfg(test)]
mod tests {
    use domain::known_secrets::KnownSecrets;
    use domain::source_failure::SourceFailure;
    use domain::source_outcomes::SourceOutcomes;
    use domain::{MergeReadiness, PrKind, PullRequest, RepoSlug, Urgency};
    use rstest::rstest;

    use super::*;
    use crate::status::StatusReport;

    fn pr(number: u64, kind: PrKind) -> PullRequest {
        PullRequest {
            number,
            title: "a title".to_string(),
            repo: RepoSlug::new("ooloth", "hub"),
            url: format!("https://github.com/ooloth/hub/pull/{number}"),
            age: chrono::Duration::zero(),
            urgency: Urgency::High,
            kind,
            author: "someone".to_string(),
            review_decision: None,
            approval_count: 1,
            changes_requested_count: 0,
            comment_count: 0,
            head_branch: "h".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_readiness: MergeReadiness::Ready,
        }
    }

    fn refresh(items: Vec<StatusItem>, answered: &[&str], failed: &[&str]) -> Refresh {
        let no_secrets = KnownSecrets::new(Vec::new());
        Refresh {
            report: StatusReport {
                items,
                errors: failed.iter().map(|name| (*name).to_string()).collect(),
            },
            sources: SourceOutcomes::new(
                answered.iter().map(|name| (*name).to_string()).collect(),
                failed
                    .iter()
                    .map(|name| SourceFailure::new(*name, &anyhow::anyhow!("down"), &no_secrets))
                    .collect(),
            ),
        }
    }

    #[test]
    fn only_pull_requests_waiting_on_me_are_turns() {
        let seen = turns_seen(&refresh(
            vec![
                StatusItem::Pr(pr(1, PrKind::ToReview)),
                StatusItem::Pr(pr(2, PrKind::Mine)),
                StatusItem::Pr(pr(3, PrKind::External)),
            ],
            &[MY_OPEN_PRS, PRS_AWAITING_REVIEW],
            &[],
        ));

        let numbers: Vec<u64> = seen.turns().keys().map(|pr| pr.number).collect();
        assert_eq!(numbers, vec![1, 2]);
    }

    #[rstest]
    #[case::both_answered(&[MY_OPEN_PRS, PRS_AWAITING_REVIEW, "linear issues"], &[], TurnCoverage::Complete)]
    #[case::other_sources_failing_do_not_matter(&[MY_OPEN_PRS, PRS_AWAITING_REVIEW], &["linear issues"], TurnCoverage::Complete)]
    #[case::my_prs_failed(&[PRS_AWAITING_REVIEW], &[MY_OPEN_PRS], TurnCoverage::Incomplete)]
    #[case::review_requests_failed(&[MY_OPEN_PRS], &[PRS_AWAITING_REVIEW], TurnCoverage::Incomplete)]
    #[case::nothing_answered(&[], &[MY_OPEN_PRS, PRS_AWAITING_REVIEW], TurnCoverage::Incomplete)]
    fn coverage_is_complete_only_when_both_pull_request_sources_answered(
        #[case] answered: &[&str],
        #[case] failed: &[&str],
        #[case] expected: TurnCoverage,
    ) {
        let seen = turns_seen(&refresh(vec![], answered, failed));

        assert_eq!(seen.coverage(), expected);
    }
}
