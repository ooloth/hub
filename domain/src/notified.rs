//! Which pull requests have a notification showing, and what to change when the turns change.
//!
//! A pull request is notified once, when it becomes my turn, and its notification is removed when
//! the turn ends. Posting again would present the card as a new interruption, so a pull request
//! that stays my turn is never posted again, even after its card is dismissed.
use std::collections::BTreeSet;

use crate::pr_id::PrId;
use crate::pr_turn::Turn;
use crate::turns_seen::{TurnCoverage, TurnsSeen};

/// The pull requests whose notification was posted, or found showing at startup, and not since
/// removed.
///
/// Only which pull requests, for now. A reminder would need when each was posted, and snoozing
/// until when it is snoozed: those become values here, keyed by pull request, and nothing outside
/// this type changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notified {
    prs: BTreeSet<PrId>,
}

/// What to post and what to remove so the notifications match the turns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationChanges {
    /// Turns that have no notification yet.
    pub post: Vec<Turn>,
    /// Notified pull requests whose turn has ended.
    pub remove: Vec<PrId>,
}

/// What happened to one change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeOutcome {
    /// The notification was posted.
    Posted(PrId),
    /// It could not be posted, so the next pass tries again.
    PostFailed {
        /// The pull request.
        pr: PrId,
        /// Why.
        reason: String,
    },
    /// The notification was removed.
    Removed(PrId),
    /// It could not be removed, so the next pass tries again.
    RemoveFailed {
        /// The pull request.
        pr: PrId,
        /// Why.
        reason: String,
    },
}

impl Notified {
    /// The pull requests whose notifications were found showing.
    #[must_use]
    pub fn from_showing(prs: impl IntoIterator<Item = PrId>) -> Self {
        Self {
            prs: prs.into_iter().collect(),
        }
    }

    /// Whether `pr` has a notification showing, as far as hub knows.
    #[must_use]
    pub fn contains(&self, pr: &PrId) -> bool {
        self.prs.contains(pr)
    }

    /// The notified pull requests.
    pub fn prs(&self) -> impl Iterator<Item = &PrId> {
        self.prs.iter()
    }

    /// What to post and remove so the notifications match `seen`.
    ///
    /// Posts each turn not yet notified. Removes each notified pull request `seen` no longer
    /// counts as a turn, but only when every source that reports a turn answered, and never one
    /// `seen` could not decide.
    ///
    /// # Panics
    /// When the changes contradict the record or `seen`, which only a bug here can cause.
    #[must_use]
    pub fn changes_for(&self, seen: &TurnsSeen) -> NotificationChanges {
        let post: Vec<Turn> = seen
            .turns()
            .values()
            .filter(|turn| !self.contains(&turn.pr))
            .cloned()
            .collect();
        let remove: Vec<PrId> = match seen.coverage() {
            TurnCoverage::Complete => self
                .prs()
                .filter(|pr| !seen.turns().contains_key(*pr) && !seen.is_undecided(pr))
                .cloned()
                .collect(),
            TurnCoverage::Incomplete => Vec::new(),
        };

        assert!(
            post.iter().all(|turn| !self.contains(&turn.pr)),
            "posting a pull request that is already notified: {post:?}"
        );
        assert!(
            remove.iter().all(|pr| self.contains(pr)),
            "removing a pull request that is not notified: {remove:?}"
        );
        assert!(
            remove.iter().all(|pr| !seen.turns().contains_key(pr)),
            "removing a pull request that is still my turn: {remove:?}"
        );
        assert!(
            seen.coverage() == TurnCoverage::Complete || remove.is_empty(),
            "removing with a source missing: {remove:?}"
        );
        NotificationChanges { post, remove }
    }

    /// The record once `outcomes` have happened. A change that failed leaves the record as it was,
    /// so the next pass tries it again.
    #[must_use]
    pub fn record(self, outcomes: &[ChangeOutcome]) -> Self {
        let mut prs = self.prs;
        for outcome in outcomes {
            match outcome {
                ChangeOutcome::Posted(pr) => {
                    let _ = prs.insert(pr.clone());
                }
                ChangeOutcome::Removed(pr) => {
                    let _ = prs.remove(pr);
                }
                ChangeOutcome::PostFailed { .. } | ChangeOutcome::RemoveFailed { .. } => {}
            }
        }
        Self { prs }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;
    use crate::pr::RepoSlug;
    use crate::pr_turn::TurnReason;
    use crate::untrusted::UntrustedText;

    fn id(number: u64) -> PrId {
        PrId {
            repo: RepoSlug::new("ooloth", "hub"),
            number,
        }
    }

    fn turn(number: u64) -> Turn {
        Turn {
            reason: TurnReason::ReviewRequested,
            pr: id(number),
            author: "someone".to_string(),
            title: UntrustedText::new("a title"),
            url: format!("https://github.com/ooloth/hub/pull/{number}"),
        }
    }

    fn seen(turns: &[u64], undecided: &[u64], coverage: TurnCoverage) -> TurnsSeen {
        TurnsSeen::new(
            turns.iter().map(|n| (id(*n), turn(*n))).collect(),
            undecided.iter().map(|n| id(*n)).collect(),
            coverage,
        )
    }

    fn notified(numbers: &[u64]) -> Notified {
        Notified::from_showing(numbers.iter().map(|n| id(*n)))
    }

    fn numbers<'a>(prs: impl IntoIterator<Item = &'a PrId>) -> BTreeSet<u64> {
        prs.into_iter().map(|pr| pr.number).collect()
    }

    /// Pull request numbers from a small range, so generated sets overlap often.
    fn some_numbers() -> impl Strategy<Value = BTreeSet<u64>> {
        prop::collection::btree_set(1..12_u64, 0..8)
    }

    fn coverage() -> impl Strategy<Value = TurnCoverage> {
        prop_oneof![Just(TurnCoverage::Complete), Just(TurnCoverage::Incomplete)]
    }

    proptest! {
        #[test]
        fn posts_are_the_turns_not_yet_notified(
            shown in some_numbers(),
            turns in some_numbers(),
            undecided in some_numbers(),
            coverage in coverage(),
        ) {
            let undecided: BTreeSet<u64> = undecided.difference(&turns).copied().collect();
            let turns_list: Vec<u64> = turns.iter().copied().collect();
            let undecided_list: Vec<u64> = undecided.iter().copied().collect();
            let shown_list: Vec<u64> = shown.iter().copied().collect();

            let changes = notified(&shown_list).changes_for(&seen(&turns_list, &undecided_list, coverage));

            let posted = numbers(changes.post.iter().map(|turn| &turn.pr));
            prop_assert_eq!(posted, turns.difference(&shown).copied().collect::<BTreeSet<_>>());
        }

        #[test]
        fn with_every_source_answering_removals_are_the_notified_pull_requests_no_longer_turns(
            shown in some_numbers(),
            turns in some_numbers(),
            undecided in some_numbers(),
        ) {
            let undecided: BTreeSet<u64> = undecided.difference(&turns).copied().collect();
            let turns_list: Vec<u64> = turns.iter().copied().collect();
            let undecided_list: Vec<u64> = undecided.iter().copied().collect();
            let shown_list: Vec<u64> = shown.iter().copied().collect();

            let changes = notified(&shown_list)
                .changes_for(&seen(&turns_list, &undecided_list, TurnCoverage::Complete));

            let expected: BTreeSet<u64> = shown
                .difference(&turns)
                .filter(|n| !undecided.contains(n))
                .copied()
                .collect();
            prop_assert_eq!(numbers(&changes.remove), expected);
        }

        #[test]
        fn with_a_source_missing_nothing_is_removed(
            shown in some_numbers(),
            turns in some_numbers(),
        ) {
            let turns_list: Vec<u64> = turns.iter().copied().collect();
            let shown_list: Vec<u64> = shown.iter().copied().collect();

            let changes = notified(&shown_list)
                .changes_for(&seen(&turns_list, &[], TurnCoverage::Incomplete));

            prop_assert!(changes.remove.is_empty());
        }

        #[test]
        fn recording_keeps_what_was_posted_and_drops_what_was_removed_and_nothing_else(
            shown in some_numbers(),
            turns in some_numbers(),
            failed in some_numbers(),
        ) {
            let turns_list: Vec<u64> = turns.iter().copied().collect();
            let shown_list: Vec<u64> = shown.iter().copied().collect();
            let record = notified(&shown_list);
            let changes = record.changes_for(&seen(&turns_list, &[], TurnCoverage::Complete));
            let outcomes: Vec<ChangeOutcome> = changes
                .post
                .iter()
                .map(|turn| if failed.contains(&turn.pr.number) {
                    ChangeOutcome::PostFailed { pr: turn.pr.clone(), reason: "timed out".to_string() }
                } else {
                    ChangeOutcome::Posted(turn.pr.clone())
                })
                .chain(changes.remove.iter().map(|pr| if failed.contains(&pr.number) {
                    ChangeOutcome::RemoveFailed { pr: pr.clone(), reason: "timed out".to_string() }
                } else {
                    ChangeOutcome::Removed(pr.clone())
                }))
                .collect();

            let after = numbers(record.record(&outcomes).prs());

            // Posted turns join, removed ones leave, and a failure leaves its pull request as it
            // was: a failed post stays out and a failed removal stays in.
            let expected: BTreeSet<u64> = shown
                .union(&turns)
                .filter(|n| turns.contains(n) && (shown.contains(n) || !failed.contains(n))
                    || !turns.contains(n) && failed.contains(n))
                .copied()
                .collect();
            prop_assert_eq!(after, expected);
        }
    }

    #[test]
    fn a_turn_whose_card_was_dismissed_is_not_posted_again() {
        let record = notified(&[]);
        let first = seen(&[7], &[], TurnCoverage::Complete);
        let changes = record.changes_for(&first);
        let record = record.record(&[ChangeOutcome::Posted(id(7))]);
        assert_eq!(changes.post.len(), 1);

        // Dismissing the card changes nothing hub can see, and the pull request is still my turn.
        let next = record.changes_for(&seen(&[7], &[], TurnCoverage::Complete));

        assert_eq!(next, NotificationChanges::default());
    }

    #[test]
    fn a_failed_post_is_tried_again_on_the_next_pass() {
        let record = notified(&[]).record(&[ChangeOutcome::PostFailed {
            pr: id(7),
            reason: "timed out".to_string(),
        }]);

        let next = record.changes_for(&seen(&[7], &[], TurnCoverage::Complete));

        assert_eq!(
            numbers(next.post.iter().map(|turn| &turn.pr)),
            BTreeSet::from([7])
        );
    }

    #[test]
    fn a_pull_request_github_has_not_decided_keeps_its_card() {
        let record = notified(&[7]);

        let changes = record.changes_for(&seen(&[], &[7], TurnCoverage::Complete));

        assert!(changes.remove.is_empty(), "{changes:?}");
    }
}
