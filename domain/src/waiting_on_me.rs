//! Which pull requests the card counts as waiting on me.
//!
//! A pull request is counted from the refresh that finds it is my turn, and stops being counted
//! only when a refresh that heard from every pull request source says its turn ended. So a source
//! that fails leaves the count as it was rather than shrinking it.
use std::collections::BTreeMap;

use crate::pr_id::PrId;
use crate::reason_kind::ReasonKind;
use crate::turns_seen::{TurnCoverage, TurnsSeen};

/// The pull requests counted as waiting on me, each with the kind of reason it last had.
///
/// A reminder would need when each began, and snoozing until when it is snoozed: those join what is
/// kept per pull request here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WaitingOnMe {
    prs: BTreeMap<PrId, ReasonKind>,
}

impl WaitingOnMe {
    /// What is counted once `seen` has arrived.
    ///
    /// Every turn in `seen` is counted with its reason. When every source answered, a pull request
    /// that is neither a turn nor undecided stops being counted. When one did not, nothing stops.
    ///
    /// # Panics
    /// When the result contradicts `seen` or what was counted before, which only a bug here can
    /// cause.
    #[must_use]
    pub fn after(self, seen: &TurnsSeen) -> Self {
        let before = self.prs;
        let mut prs = match seen.coverage() {
            TurnCoverage::Complete => before
                .iter()
                .filter(|(pr, _)| seen.is_undecided(pr))
                .map(|(pr, kind)| (pr.clone(), *kind))
                .collect(),
            TurnCoverage::Incomplete => before.clone(),
        };
        for (pr, turn) in seen.turns() {
            let _ = prs.insert(pr.clone(), ReasonKind::of(&turn.reason));
        }

        assert!(
            seen.turns()
                .iter()
                .all(|(pr, turn)| prs.get(pr) == Some(&ReasonKind::of(&turn.reason))),
            "a turn seen is not counted under its reason: {prs:?}"
        );
        assert!(
            seen.coverage() == TurnCoverage::Incomplete
                || prs
                    .keys()
                    .all(|pr| seen.turns().contains_key(pr) || seen.is_undecided(pr)),
            "counting a pull request a complete refresh says is not my turn: {prs:?}"
        );
        assert!(
            seen.coverage() == TurnCoverage::Complete
                || before.keys().all(|pr| prs.contains_key(pr)),
            "an incomplete refresh dropped a pull request: {prs:?}"
        );
        Self { prs }
    }

    /// The pull requests counted.
    pub fn prs(&self) -> impl Iterator<Item = &PrId> {
        self.prs.keys()
    }

    /// The kind of reason each counted pull request has.
    pub fn reasons(&self) -> impl Iterator<Item = ReasonKind> + '_ {
        self.prs.values().copied()
    }

    /// How many pull requests are counted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.prs.len()
    }

    /// Whether nothing is counted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.prs.is_empty()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;
    use crate::pr::RepoSlug;
    use crate::pr_turn::{Turn, TurnReason};

    pub(crate) fn id(number: u64) -> PrId {
        PrId {
            repo: RepoSlug::new("ooloth", "hub"),
            number,
        }
    }

    /// What a refresh with `coverage` saw: `turns` with their reasons, and `undecided`.
    pub(crate) fn seen(
        turns: &BTreeMap<u64, TurnReason>,
        undecided: &BTreeSet<u64>,
        coverage: TurnCoverage,
    ) -> TurnsSeen {
        TurnsSeen::new(
            turns
                .iter()
                .map(|(n, reason)| {
                    (
                        id(*n),
                        Turn {
                            reason: reason.clone(),
                            pr: id(*n),
                        },
                    )
                })
                .collect(),
            undecided
                .iter()
                .filter(|n| !turns.contains_key(n))
                .map(|n| id(*n))
                .collect(),
            coverage,
        )
    }

    pub(crate) fn turns(cases: &[(u64, TurnReason)]) -> BTreeMap<u64, TurnReason> {
        cases.iter().cloned().collect()
    }

    fn numbers(waiting: &WaitingOnMe) -> BTreeSet<u64> {
        waiting.prs().map(|pr| pr.number).collect()
    }

    pub(crate) fn reason() -> impl Strategy<Value = TurnReason> {
        prop_oneof![
            Just(TurnReason::ReviewRequested),
            Just(TurnReason::Feedback {
                by: "ada".to_string()
            }),
            Just(TurnReason::ChecksFailing),
            Just(TurnReason::Conflicts),
            Just(TurnReason::ReadyToMerge),
            Just(TurnReason::NoReviewer),
            Just(TurnReason::Draft),
        ]
    }

    /// Pull request numbers from a small range, so generated refreshes overlap often.
    pub(crate) fn some_turns() -> impl Strategy<Value = BTreeMap<u64, TurnReason>> {
        prop::collection::btree_map(1..12_u64, reason(), 0..8)
    }

    fn some_numbers() -> impl Strategy<Value = BTreeSet<u64>> {
        prop::collection::btree_set(1..12_u64, 0..8)
    }

    pub(crate) fn coverage() -> impl Strategy<Value = TurnCoverage> {
        prop_oneof![Just(TurnCoverage::Complete), Just(TurnCoverage::Incomplete)]
    }

    proptest! {
        #[test]
        fn every_turn_a_refresh_sees_is_counted_under_its_reason(
            before in some_turns(),
            now in some_turns(),
            undecided in some_numbers(),
            coverage in coverage(),
        ) {
            let earlier = WaitingOnMe::default().after(&seen(&before, &BTreeSet::new(), TurnCoverage::Complete));

            let waiting = earlier.after(&seen(&now, &undecided, coverage));

            for (n, reason) in &now {
                prop_assert_eq!(waiting.prs.get(&id(*n)), Some(&ReasonKind::of(reason)));
            }
        }

        #[test]
        fn a_complete_refresh_drops_every_pull_request_that_is_neither_a_turn_nor_undecided(
            before in some_turns(),
            now in some_turns(),
            undecided in some_numbers(),
        ) {
            let earlier = WaitingOnMe::default().after(&seen(&before, &BTreeSet::new(), TurnCoverage::Complete));

            let waiting = earlier.after(&seen(&now, &undecided, TurnCoverage::Complete));

            let expected: BTreeSet<u64> = now
                .keys()
                .copied()
                .chain(before.keys().copied().filter(|n| undecided.contains(n)))
                .collect();
            prop_assert_eq!(numbers(&waiting), expected);
        }

        #[test]
        fn an_incomplete_refresh_drops_nothing_that_was_counted(
            before in some_turns(),
            now in some_turns(),
        ) {
            let earlier = WaitingOnMe::default().after(&seen(&before, &BTreeSet::new(), TurnCoverage::Complete));

            let waiting = earlier.after(&seen(&now, &BTreeSet::new(), TurnCoverage::Incomplete));

            let expected: BTreeSet<u64> = before.keys().chain(now.keys()).copied().collect();
            prop_assert_eq!(numbers(&waiting), expected);
        }
    }

    #[test]
    fn an_undecided_pull_request_keeps_its_last_reason_through_a_complete_refresh() {
        let earlier = WaitingOnMe::default().after(&seen(
            &turns(&[(7, TurnReason::ReadyToMerge)]),
            &BTreeSet::new(),
            TurnCoverage::Complete,
        ));

        let waiting = earlier.after(&seen(
            &BTreeMap::new(),
            &BTreeSet::from([7]),
            TurnCoverage::Complete,
        ));

        assert_eq!(waiting.prs.get(&id(7)), Some(&ReasonKind::ReadyToMerge));
    }
}
