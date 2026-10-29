//! Why a pull request is waiting on me, as the card counts it.
use std::num::NonZeroUsize;

use crate::pr_turn::TurnReason;

/// Why a pull request is waiting on me, without who or what caused it.
///
/// Declared in the order the card lists its rows: first where someone else waits on me, then work
/// I can finish, then my drafts. The collapsed card shows only its first three rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReasonKind {
    /// Someone asked for my review.
    ReviewRequested,
    /// It is mine, and someone left something that asks something of me.
    Feedback,
    /// It is mine, and a check failed.
    ChecksFailing,
    /// It is mine, approved, and its branch conflicts with its base.
    Conflicts,
    /// It is mine, approved, and nothing stops the merge.
    ReadyToMerge,
    /// It is mine, and nobody else is asked to review it.
    NoReviewer,
    /// It is mine, and still a draft.
    Draft,
}

impl ReasonKind {
    /// The kind of `reason`.
    #[must_use]
    pub const fn of(reason: &TurnReason) -> Self {
        match reason {
            TurnReason::ReviewRequested => Self::ReviewRequested,
            TurnReason::Feedback { .. } => Self::Feedback,
            TurnReason::ChecksFailing => Self::ChecksFailing,
            TurnReason::Conflicts => Self::Conflicts,
            TurnReason::ReadyToMerge => Self::ReadyToMerge,
            TurnReason::NoReviewer => Self::NoReviewer,
            TurnReason::Draft => Self::Draft,
        }
    }

    /// The card's row for `count` pull requests waiting on me for this reason.
    #[must_use]
    pub fn row(self, count: NonZeroUsize) -> String {
        let one = count.get() == 1;
        let words = match (self, one) {
            (Self::ReviewRequested, true) => "needs your review",
            (Self::ReviewRequested, false) => "need your review",
            (Self::Feedback, true) => "has feedback",
            (Self::Feedback, false) => "have feedback",
            (Self::ChecksFailing, true) => "has failing checks",
            (Self::ChecksFailing, false) => "have failing checks",
            (Self::Conflicts, true) => "has a merge conflict",
            (Self::Conflicts, false) => "have merge conflicts",
            (Self::ReadyToMerge, true) => "is ready to merge",
            (Self::ReadyToMerge, false) => "are ready to merge",
            (Self::NoReviewer, true) => "hasn't requested reviews",
            (Self::NoReviewer, false) => "haven't requested reviews",
            (Self::Draft, true) => "is still a draft",
            (Self::Draft, false) => "are still drafts",
        };
        format!("{count} {words}")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(TurnReason::ReviewRequested, ReasonKind::ReviewRequested)]
    #[case(TurnReason::Feedback { by: "ada".to_string() }, ReasonKind::Feedback)]
    #[case(TurnReason::ChecksFailing, ReasonKind::ChecksFailing)]
    #[case(TurnReason::Conflicts, ReasonKind::Conflicts)]
    #[case(TurnReason::ReadyToMerge, ReasonKind::ReadyToMerge)]
    #[case(TurnReason::NoReviewer, ReasonKind::NoReviewer)]
    #[case(TurnReason::Draft, ReasonKind::Draft)]
    fn each_reason_has_the_kind_of_the_same_name(
        #[case] reason: TurnReason,
        #[case] kind: ReasonKind,
    ) {
        assert_eq!(ReasonKind::of(&reason), kind);
    }

    #[test]
    fn feedback_from_anyone_is_the_same_kind() {
        assert_eq!(
            ReasonKind::of(&TurnReason::Feedback {
                by: "ada".to_string()
            }),
            ReasonKind::of(&TurnReason::Feedback {
                by: "bo".to_string()
            })
        );
    }
}
