//! The one card that says how many pull requests are waiting on me, and why.
//!
//! The card is posted after every refresh while anything is counted, so it shows again each time,
//! and is removed once a refresh that heard from every source leaves nothing counted. It names no
//! pull request.
use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use crate::reason_kind::ReasonKind;
use crate::turns_seen::TurnCoverage;
use crate::waiting_on_me::WaitingOnMe;

/// What the card says when the refresh behind it missed a pull request source. Every source a
/// turn depends on is a GitHub query.
pub const INCOMPLETE_NOTE: &str = "GitHub didn't provide complete results";

/// How many pull requests are waiting on me for each reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitingCard {
    rows: BTreeMap<ReasonKind, NonZeroUsize>,
    coverage: TurnCoverage,
}

impl WaitingCard {
    /// How many pull requests the card counts.
    #[must_use]
    pub fn total(&self) -> usize {
        self.rows.values().map(|count| count.get()).sum()
    }

    /// "4 PRs are waiting for you", or "1 PR is waiting for you".
    #[must_use]
    pub fn title(&self) -> String {
        match self.total() {
            1 => "1 PR is waiting for you".to_string(),
            total => format!("{total} PRs are waiting for you"),
        }
    }

    /// One row per reason, in the card's order.
    #[must_use]
    pub fn message(&self) -> String {
        self.rows
            .iter()
            .map(|(kind, count)| kind.row(*count))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Why the count may be short, when it may be.
    #[must_use]
    pub const fn note(&self) -> Option<&'static str> {
        match self.coverage {
            TurnCoverage::Complete => None,
            TurnCoverage::Incomplete => Some(INCOMPLETE_NOTE),
        }
    }
}

/// What to do with the card after a refresh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardChange {
    /// Post this card, replacing the one showing, so it shows again.
    Show(WaitingCard),
    /// Every source answered and nothing is waiting on me: remove the card.
    Remove,
    /// Nothing is counted, but a source did not answer: leave the card as it is.
    Leave,
}

impl CardChange {
    /// What to do with the card now that `waiting` counts what the latest refresh, whose
    /// `coverage` is given, and earlier refreshes found.
    ///
    /// # Panics
    /// When the change contradicts `waiting` or `coverage`, which only a bug here can cause.
    #[must_use]
    pub fn after(waiting: &WaitingOnMe, coverage: TurnCoverage) -> Self {
        let mut rows: BTreeMap<ReasonKind, NonZeroUsize> = BTreeMap::new();
        for kind in waiting.reasons() {
            let _ = rows
                .entry(kind)
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(NonZeroUsize::MIN);
        }
        let change = match (rows.is_empty(), coverage) {
            (false, coverage) => Self::Show(WaitingCard { rows, coverage }),
            (true, TurnCoverage::Complete) => Self::Remove,
            (true, TurnCoverage::Incomplete) => Self::Leave,
        };

        match &change {
            Self::Show(card) => assert_eq!(
                card.total(),
                waiting.len(),
                "the card's total is not the number of pull requests counted"
            ),
            Self::Remove => {
                assert!(
                    waiting.is_empty(),
                    "removing the card while something is counted"
                );
                assert_eq!(
                    coverage,
                    TurnCoverage::Complete,
                    "removing the card while a source is missing"
                );
            }
            Self::Leave => assert!(
                waiting.is_empty(),
                "leaving the card while something is counted"
            ),
        }
        change
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::pr_turn::TurnReason;
    use crate::waiting_on_me::tests::{coverage, seen, some_turns, turns};

    fn change(cases: &[(u64, TurnReason)], coverage: TurnCoverage) -> CardChange {
        let seen = seen(&turns(cases), &BTreeSet::new(), coverage);
        CardChange::after(&WaitingOnMe::default().after(&seen), seen.coverage())
    }

    fn shown(change: CardChange) -> WaitingCard {
        match change {
            CardChange::Show(card) => card,
            CardChange::Remove | CardChange::Leave => panic!("expected a card, got {change:?}"),
        }
    }

    /// The card as it reads: title, the note when there is one, then the rows.
    fn rendered(card: &WaitingCard) -> String {
        let note = card
            .note()
            .map(|note| format!("{note}\n"))
            .unwrap_or_default();
        format!("{}\n{note}{}", card.title(), card.message())
    }

    proptest! {
        #[test]
        fn a_cards_rows_add_up_to_the_pull_requests_counted(
            before in some_turns(),
            now in some_turns(),
            coverage in coverage(),
        ) {
            let earlier = WaitingOnMe::default().after(&seen(&before, &BTreeSet::new(), TurnCoverage::Complete));
            let now = seen(&now, &BTreeSet::new(), coverage);
            let waiting = earlier.after(&now);

            if let CardChange::Show(card) = CardChange::after(&waiting, now.coverage()) {
                let sum: usize = card.rows.values().map(|count| count.get()).sum();
                prop_assert_eq!(sum, waiting.len());
                prop_assert_eq!(card.total(), waiting.len());
            } else {
                prop_assert!(waiting.is_empty());
            }
        }
    }

    #[test]
    fn the_same_turns_on_two_refreshes_show_the_card_both_times() {
        let refresh = seen(
            &turns(&[(1, TurnReason::ReviewRequested)]),
            &BTreeSet::new(),
            TurnCoverage::Complete,
        );
        let first = WaitingOnMe::default().after(&refresh);
        let first_change = CardChange::after(&first, refresh.coverage());
        let second = first.after(&refresh);

        let second_change = CardChange::after(&second, refresh.coverage());

        assert!(
            matches!(first_change, CardChange::Show(_)),
            "{first_change:?}"
        );
        assert_eq!(second_change, first_change);
    }

    #[test]
    fn a_complete_refresh_with_nothing_counted_removes_the_card() {
        assert_eq!(change(&[], TurnCoverage::Complete), CardChange::Remove);
    }

    #[test]
    fn an_incomplete_refresh_with_nothing_counted_leaves_the_card() {
        assert_eq!(change(&[], TurnCoverage::Incomplete), CardChange::Leave);
    }

    #[test]
    fn a_card_from_an_incomplete_refresh_says_github_did_not_provide_complete_results() {
        let card = shown(change(
            &[(1, TurnReason::ReviewRequested)],
            TurnCoverage::Incomplete,
        ));

        assert_eq!(card.note(), Some("GitHub didn't provide complete results"));
    }

    #[test]
    fn a_card_from_a_complete_refresh_has_no_note() {
        let card = shown(change(
            &[(1, TurnReason::ReviewRequested)],
            TurnCoverage::Complete,
        ));

        assert_eq!(card.note(), None);
    }

    #[rstest]
    #[case::one(1, "1 PR is waiting for you")]
    #[case::several(3, "3 PRs are waiting for you")]
    fn the_title_says_how_many_are_waiting(#[case] count: u64, #[case] title: &str) {
        let cases: Vec<(u64, TurnReason)> = (1..=count)
            .map(|n| (n, TurnReason::ReviewRequested))
            .collect();

        assert_eq!(shown(change(&cases, TurnCoverage::Complete)).title(), title);
    }

    fn every_reason() -> [TurnReason; 7] {
        [
            TurnReason::Draft,
            TurnReason::NoReviewer,
            TurnReason::ReadyToMerge,
            TurnReason::Conflicts,
            TurnReason::ChecksFailing,
            TurnReason::Feedback {
                by: "ada".to_string(),
            },
            TurnReason::ReviewRequested,
        ]
    }

    #[test]
    fn each_reason_reads_as_one_row_for_a_single_pull_request_in_the_cards_order() {
        let cases: Vec<(u64, TurnReason)> = every_reason()
            .into_iter()
            .zip(1..)
            .map(|(reason, n)| (n, reason))
            .collect();

        let card = shown(change(&cases, TurnCoverage::Complete));

        assert_eq!(
            rendered(&card),
            "7 PRs are waiting for you\n\
             1 needs your review\n\
             1 has feedback\n\
             1 has failing checks\n\
             1 has a merge conflict\n\
             1 is ready to merge\n\
             1 hasn't requested reviews\n\
             1 is still a draft"
        );
    }

    #[test]
    fn each_reason_reads_as_one_row_for_several_pull_requests_in_the_cards_order() {
        let cases: Vec<(u64, TurnReason)> = every_reason()
            .into_iter()
            .chain(every_reason())
            .zip(1..)
            .map(|(reason, n)| (n, reason))
            .collect();

        let card = shown(change(&cases, TurnCoverage::Incomplete));

        assert_eq!(
            rendered(&card),
            "14 PRs are waiting for you\n\
             GitHub didn't provide complete results\n\
             2 need your review\n\
             2 have feedback\n\
             2 have failing checks\n\
             2 have merge conflicts\n\
             2 are ready to merge\n\
             2 haven't requested reviews\n\
             2 are still drafts"
        );
    }

    #[test]
    fn a_reason_with_no_pull_request_has_no_row() {
        let card = shown(change(
            &[(1, TurnReason::Draft), (2, TurnReason::Draft)],
            TurnCoverage::Complete,
        ));

        assert_eq!(card.message(), "2 are still drafts");
        assert_eq!(
            card.rows,
            BTreeMap::from([(ReasonKind::Draft, NonZeroUsize::new(2).unwrap())])
        );
    }
}
