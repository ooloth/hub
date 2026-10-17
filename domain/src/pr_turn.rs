//! Whether a pull request is waiting on me, and the card that says so.
use crate::pr::{MergeReadiness, PrKind, PullRequest, ReviewStatus};
use crate::pr_id::PrId;
use crate::untrusted::UntrustedText;

/// Why a pull request is waiting on me.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnReason {
    /// Someone asked for my review.
    ReviewRequested,
    /// It is mine, approved, and nothing stops the merge.
    ReadyToMerge,
}

/// A pull request that is waiting on me, with what a notification about it needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    /// Why it is waiting on me.
    pub reason: TurnReason,
    /// Which pull request it is.
    pub pr: PrId,
    /// Who opened it.
    pub author: String,
    /// Its title, as its author wrote it.
    pub title: UntrustedText,
    /// Where it is on GitHub.
    pub url: String,
}

/// What one refresh says about whether a pull request is waiting on me.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnReading {
    /// It is.
    Turn(Turn),
    /// It is not.
    NotMyTurn,
    /// It would be if nothing stops the merge, and GitHub has not worked that out yet.
    Undecided,
}

impl TurnReading {
    /// Whether `pr` is waiting on me: my review is requested, or it is mine, approved, and nothing
    /// stops the merge.
    #[must_use]
    pub fn of(pr: &PullRequest) -> Self {
        let reason = match pr.kind {
            PrKind::ToReview => TurnReason::ReviewRequested,
            PrKind::Mine => {
                if !matches!(pr.review_status(), ReviewStatus::Approved(_)) {
                    return Self::NotMyTurn;
                }
                match pr.merge_readiness {
                    MergeReadiness::Ready => TurnReason::ReadyToMerge,
                    MergeReadiness::Blocked(_) => return Self::NotMyTurn,
                    MergeReadiness::NotYetKnown => return Self::Undecided,
                }
            }
            PrKind::MyDraft | PrKind::External => return Self::NotMyTurn,
        };
        Self::Turn(Turn {
            reason,
            pr: PrId::of(pr),
            author: pr.author.clone(),
            title: UntrustedText::new(pr.title.clone()),
            url: pr.url.clone(),
        })
    }
}

/// What a notification about a turn says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnCard {
    /// Why it is waiting on me.
    pub title: String,
    /// Which pull request, and who opened it when that is not me.
    pub subtitle: String,
    /// The pull request's title.
    pub message: UntrustedText,
    /// What clicking the notification opens.
    pub url: String,
}

impl Turn {
    /// The notification for this turn.
    #[must_use]
    pub fn card(&self) -> TurnCard {
        let repo_and_number = format!("{} #{}", self.pr.repo.repo_name(), self.pr.number);
        let (title, subtitle) = match self.reason {
            TurnReason::ReviewRequested => (
                "PR review requested",
                format!("{repo_and_number} · @{}", self.author),
            ),
            TurnReason::ReadyToMerge => ("PR ready to merge", repo_and_number),
        };
        TurnCard {
            title: title.to_string(),
            subtitle,
            message: self.title.clone(),
            url: self.url.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use rstest::rstest;

    use super::*;
    use crate::pr::{MergeBlocker, RepoSlug, ReviewDecision};
    use crate::urgency::Urgency;

    fn pr(
        kind: PrKind,
        approvals: u32,
        changes_requested: u32,
        merge: MergeReadiness,
    ) -> PullRequest {
        PullRequest {
            number: 344,
            title: "[WIP] Notify when a pull request is my turn".to_string(),
            repo: RepoSlug::new("ooloth", "hub"),
            url: "https://github.com/ooloth/hub/pull/344".to_string(),
            age: Duration::zero(),
            urgency: Urgency::High,
            kind,
            author: "someone".to_string(),
            review_decision: None,
            approval_count: approvals,
            changes_requested_count: changes_requested,
            comment_count: 0,
            head_branch: "notify".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_readiness: merge,
        }
    }

    fn reason(reading: &TurnReading) -> Option<TurnReason> {
        match reading {
            TurnReading::Turn(turn) => Some(turn.reason),
            TurnReading::NotMyTurn | TurnReading::Undecided => None,
        }
    }

    #[rstest]
    #[case::review_requested(PrKind::ToReview, 0, 0, MergeReadiness::NotYetKnown)]
    #[case::review_requested_whatever_its_state(
        PrKind::ToReview,
        1,
        1,
        MergeReadiness::Blocked(MergeBlocker::Conflict)
    )]
    fn a_review_request_is_my_turn(
        #[case] kind: PrKind,
        #[case] approvals: u32,
        #[case] changes_requested: u32,
        #[case] merge: MergeReadiness,
    ) {
        let reading = TurnReading::of(&pr(kind, approvals, changes_requested, merge));

        assert_eq!(reason(&reading), Some(TurnReason::ReviewRequested));
    }

    #[test]
    fn my_approved_pull_request_that_nothing_blocks_is_my_turn() {
        let reading = TurnReading::of(&pr(PrKind::Mine, 1, 0, MergeReadiness::Ready));

        assert_eq!(reason(&reading), Some(TurnReason::ReadyToMerge));
    }

    #[test]
    fn an_approval_github_reports_only_as_a_decision_counts() {
        let mut approved = pr(PrKind::Mine, 0, 0, MergeReadiness::Ready);
        approved.review_decision = Some(ReviewDecision::Approved);

        assert_eq!(
            reason(&TurnReading::of(&approved)),
            Some(TurnReason::ReadyToMerge)
        );
    }

    #[rstest]
    #[case::mine_without_reviews(PrKind::Mine, 0, 0, MergeReadiness::Ready)]
    #[case::mine_with_changes_requested(PrKind::Mine, 1, 1, MergeReadiness::Ready)]
    #[case::mine_with_a_conflict(
        PrKind::Mine,
        1,
        0,
        MergeReadiness::Blocked(MergeBlocker::Conflict)
    )]
    #[case::mine_behind(PrKind::Mine, 1, 0, MergeReadiness::Blocked(MergeBlocker::Behind))]
    #[case::mine_blocked_by_protection(
        PrKind::Mine,
        1,
        0,
        MergeReadiness::Blocked(MergeBlocker::Blocked)
    )]
    #[case::my_draft(PrKind::MyDraft, 1, 0, MergeReadiness::Ready)]
    #[case::someone_elses(PrKind::External, 1, 0, MergeReadiness::Ready)]
    fn other_pull_requests_are_not_my_turn(
        #[case] kind: PrKind,
        #[case] approvals: u32,
        #[case] changes_requested: u32,
        #[case] merge: MergeReadiness,
    ) {
        let reading = TurnReading::of(&pr(kind, approvals, changes_requested, merge));

        assert_eq!(reading, TurnReading::NotMyTurn);
    }

    #[test]
    fn my_approved_pull_request_is_undecided_until_github_works_out_the_merge() {
        let reading = TurnReading::of(&pr(PrKind::Mine, 1, 0, MergeReadiness::NotYetKnown));

        assert_eq!(reading, TurnReading::Undecided);
    }

    #[test]
    fn a_turn_carries_the_pull_request_it_is_about() {
        let TurnReading::Turn(turn) =
            TurnReading::of(&pr(PrKind::ToReview, 0, 0, MergeReadiness::Ready))
        else {
            panic!("expected a turn");
        };

        assert_eq!(turn.pr.to_string(), "ooloth/hub#344");
        assert_eq!(turn.author, "someone");
        assert_eq!(
            turn.title.expose(),
            "[WIP] Notify when a pull request is my turn"
        );
        assert_eq!(turn.url, "https://github.com/ooloth/hub/pull/344");
    }

    fn turn(reason: TurnReason) -> Turn {
        Turn {
            reason,
            pr: PrId {
                repo: RepoSlug::new("ooloth", "hub"),
                number: 344,
            },
            author: "someone".to_string(),
            title: UntrustedText::new("Notify when a pull request is my turn"),
            url: "https://github.com/ooloth/hub/pull/344".to_string(),
        }
    }

    #[test]
    fn a_review_request_card_names_the_repository_number_and_author() {
        let card = turn(TurnReason::ReviewRequested).card();

        assert_eq!(card.title, "PR review requested");
        assert_eq!(card.subtitle, "hub #344 · @someone");
        assert_eq!(
            card.message.expose(),
            "Notify when a pull request is my turn"
        );
        assert_eq!(card.url, "https://github.com/ooloth/hub/pull/344");
    }

    #[test]
    fn a_ready_to_merge_card_leaves_out_the_author_who_is_me() {
        let card = turn(TurnReason::ReadyToMerge).card();

        assert_eq!(card.title, "PR ready to merge");
        assert_eq!(card.subtitle, "hub #344");
    }
}
