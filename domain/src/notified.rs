//! Which pull requests have a notification showing, and what to change when the turns change.
//!
//! A pull request is notified when it becomes my turn, and its notification is removed when the
//! turn ends. Posting again for the same reason would present the card as a new interruption, so a
//! pull request that stays my turn for the same reason is never posted again, even after its card
//! is dismissed. When the reason changes, such as a review arriving on my pull request that was
//! ready to merge, the card is posted again in its group, which replaces it.
use std::collections::BTreeMap;

use crate::pr_id::PrId;
use crate::pr_turn::{Turn, TurnReason};
use crate::turns_seen::{TurnCoverage, TurnsSeen};

/// What a notified pull request's card says, as far as hub knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotifiedFor {
    /// It was posted for this reason.
    Reason(TurnReason),
    /// It was found showing at startup, for a reason hub did not record. It is not posted again
    /// while it stays my turn, since it is already on screen.
    FoundShowing,
}

/// The pull requests whose notification was posted, or found showing at startup, and not since
/// removed, with what each card says.
///
/// A reminder would need when each was posted, and snoozing until when it is snoozed: those join
/// what is kept per pull request here, and nothing outside this type changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notified {
    prs: BTreeMap<PrId, NotifiedFor>,
}

/// What to post and what to remove so the notifications match the turns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationChanges {
    /// Turns that have no notification yet.
    pub post: Vec<Turn>,
    /// Turns whose notification was posted for another reason.
    pub repost: Vec<Turn>,
    /// Notified pull requests whose turn has ended.
    pub remove: Vec<PrId>,
}

/// What happened to one change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeOutcome {
    /// The notification was posted.
    Posted {
        /// The pull request.
        pr: PrId,
        /// Why it is my turn, as the card says.
        reason: TurnReason,
    },
    /// The notification was posted again for a new reason, replacing the old card.
    Reposted {
        /// The pull request.
        pr: PrId,
        /// Why it is my turn now, as the card says.
        reason: TurnReason,
    },
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
            prs: prs
                .into_iter()
                .map(|pr| (pr, NotifiedFor::FoundShowing))
                .collect(),
        }
    }

    /// Whether `pr` has a notification showing, as far as hub knows.
    #[must_use]
    pub fn contains(&self, pr: &PrId) -> bool {
        self.prs.contains_key(pr)
    }

    /// The notified pull requests.
    pub fn prs(&self) -> impl Iterator<Item = &PrId> {
        self.prs.keys()
    }

    /// What `pr`'s card says, when it has one.
    #[must_use]
    pub fn notified_for(&self, pr: &PrId) -> Option<&NotifiedFor> {
        self.prs.get(pr)
    }

    /// What to post, post again and remove so the notifications match `seen`.
    ///
    /// Posts each turn not yet notified, and posts again each turn whose card was posted for
    /// another reason. Removes each notified pull request `seen` no longer counts as a turn, but
    /// only when every source that reports a turn answered, and never one `seen` could not decide.
    ///
    /// # Panics
    /// When the changes contradict the record or `seen`, which only a bug here can cause.
    #[must_use]
    pub fn changes_for(&self, seen: &TurnsSeen) -> NotificationChanges {
        let mut post = Vec::new();
        let mut repost = Vec::new();
        for turn in seen.turns().values() {
            match self.prs.get(&turn.pr) {
                None => post.push(turn.clone()),
                Some(NotifiedFor::Reason(reason)) if *reason != turn.reason => {
                    repost.push(turn.clone());
                }
                Some(NotifiedFor::Reason(_) | NotifiedFor::FoundShowing) => {}
            }
        }
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
            repost.iter().all(|turn| self.contains(&turn.pr)),
            "reposting a pull request that is not notified: {repost:?}"
        );
        assert!(
            repost.iter().all(|turn| self.notified_for(&turn.pr)
                != Some(&NotifiedFor::Reason(turn.reason.clone()))),
            "reposting a pull request for the reason its card already says: {repost:?}"
        );
        assert!(
            repost
                .iter()
                .all(|turn| self.notified_for(&turn.pr) != Some(&NotifiedFor::FoundShowing)),
            "reposting a pull request whose card was found showing: {repost:?}"
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
        NotificationChanges {
            post,
            repost,
            remove,
        }
    }

    /// The record once `outcomes` have happened. A change that failed leaves the record as it was,
    /// so the next pass tries it again.
    #[must_use]
    pub fn record(self, outcomes: &[ChangeOutcome]) -> Self {
        let mut prs = self.prs;
        for outcome in outcomes {
            match outcome {
                ChangeOutcome::Posted { pr, reason } | ChangeOutcome::Reposted { pr, reason } => {
                    let _ = prs.insert(pr.clone(), NotifiedFor::Reason(reason.clone()));
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
    use std::collections::{BTreeMap, BTreeSet};

    use proptest::prelude::*;

    use super::*;
    use crate::pr::RepoSlug;
    use crate::untrusted::UntrustedText;

    fn id(number: u64) -> PrId {
        PrId {
            repo: RepoSlug::new("ooloth", "hub"),
            number,
        }
    }

    fn turn(number: u64) -> Turn {
        turn_for(number, TurnReason::ReviewRequested)
    }

    fn turn_for(number: u64, reason: TurnReason) -> Turn {
        Turn {
            reason,
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

    fn seen_for(turns: &[(u64, TurnReason)]) -> TurnsSeen {
        TurnsSeen::new(
            turns
                .iter()
                .map(|(n, reason)| (id(*n), turn_for(*n, reason.clone())))
                .collect(),
            BTreeSet::new(),
            TurnCoverage::Complete,
        )
    }

    /// A record of cards posted for these reasons.
    fn posted_for(cards: &[(u64, TurnReason)]) -> Notified {
        let outcomes: Vec<ChangeOutcome> = cards
            .iter()
            .map(|(n, reason)| ChangeOutcome::Posted {
                pr: id(*n),
                reason: reason.clone(),
            })
            .collect();
        Notified::default().record(&outcomes)
    }

    fn reviewed_by(login: &str) -> TurnReason {
        TurnReason::Reviewed {
            by: login.to_string(),
        }
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

    /// A few reasons, so generated records and turns often agree and often differ.
    fn reason() -> impl Strategy<Value = TurnReason> {
        prop_oneof![
            Just(TurnReason::ReviewRequested),
            Just(TurnReason::ReadyToMerge),
            Just(reviewed_by("ada")),
            Just(reviewed_by("bo")),
        ]
    }

    fn cards() -> impl Strategy<Value = BTreeMap<u64, Option<TurnReason>>> {
        prop::collection::btree_map(1..12_u64, prop::option::of(reason()), 0..8)
    }

    fn turns_with_reasons() -> impl Strategy<Value = BTreeMap<u64, TurnReason>> {
        prop::collection::btree_map(1..12_u64, reason(), 0..8)
    }

    fn coverage() -> impl Strategy<Value = TurnCoverage> {
        prop_oneof![Just(TurnCoverage::Complete), Just(TurnCoverage::Incomplete)]
    }

    proptest! {
        #[test]
        fn exactly_the_turns_whose_card_says_another_reason_are_posted_again(
            cards in cards(),
            turns in turns_with_reasons(),
        ) {
            // A card with no reason was found showing at startup; the rest were posted.
            let found: Vec<PrId> = cards.iter().filter(|(_, r)| r.is_none()).map(|(n, _)| id(*n)).collect();
            let posted: Vec<ChangeOutcome> = cards
                .iter()
                .filter_map(|(n, r)| r.clone().map(|reason| ChangeOutcome::Posted { pr: id(*n), reason }))
                .collect();
            let record = Notified::from_showing(found).record(&posted);
            let turns_list: Vec<(u64, TurnReason)> = turns.clone().into_iter().collect();

            let changes = record.changes_for(&seen_for(&turns_list));

            let expected: BTreeSet<u64> = turns
                .iter()
                .filter(|(n, reason)| matches!(cards.get(n), Some(Some(card)) if card != *reason))
                .map(|(n, _)| *n)
                .collect();
            prop_assert_eq!(numbers(changes.repost.iter().map(|turn| &turn.pr)), expected);
            let new: BTreeSet<u64> = turns.keys().filter(|n| !cards.contains_key(n)).copied().collect();
            prop_assert_eq!(numbers(changes.post.iter().map(|turn| &turn.pr)), new);
        }

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
                    ChangeOutcome::Posted { pr: turn.pr.clone(), reason: turn.reason.clone() }
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
        let record = record.record(&[ChangeOutcome::Posted {
            pr: id(7),
            reason: TurnReason::ReviewRequested,
        }]);
        assert_eq!(changes.post.len(), 1);

        // Dismissing the card changes nothing hub can see, and the pull request is still my turn.
        let next = record.changes_for(&seen(&[7], &[], TurnCoverage::Complete));

        assert_eq!(next, NotificationChanges::default());
    }

    #[test]
    fn a_turn_whose_reason_changes_is_posted_again_once() {
        let record = posted_for(&[(7, TurnReason::ReadyToMerge)]);
        let reviewed = seen_for(&[(7, reviewed_by("ada"))]);

        let changes = record.changes_for(&reviewed);
        let record = record.record(&[ChangeOutcome::Reposted {
            pr: id(7),
            reason: reviewed_by("ada"),
        }]);

        assert_eq!(
            numbers(changes.repost.iter().map(|turn| &turn.pr)),
            BTreeSet::from([7])
        );
        assert!(changes.post.is_empty(), "{changes:?}");
        assert_eq!(
            record.changes_for(&reviewed),
            NotificationChanges::default()
        );
    }

    #[test]
    fn a_review_from_someone_else_is_a_new_reason() {
        let record = posted_for(&[(7, reviewed_by("ada"))]);

        let changes = record.changes_for(&seen_for(&[(7, reviewed_by("bo"))]));

        assert_eq!(
            numbers(changes.repost.iter().map(|turn| &turn.pr)),
            BTreeSet::from([7])
        );
    }

    #[test]
    fn a_card_found_showing_is_not_posted_again_whatever_its_reason() {
        let record = notified(&[7]);

        let changes = record.changes_for(&seen_for(&[(7, reviewed_by("ada"))]));

        assert_eq!(changes, NotificationChanges::default());
    }

    #[test]
    fn a_failed_repost_is_tried_again_on_the_next_pass() {
        let record =
            posted_for(&[(7, TurnReason::ReadyToMerge)]).record(&[ChangeOutcome::PostFailed {
                pr: id(7),
                reason: "timed out".to_string(),
            }]);

        let next = record.changes_for(&seen_for(&[(7, reviewed_by("ada"))]));

        assert_eq!(
            numbers(next.repost.iter().map(|turn| &turn.pr)),
            BTreeSet::from([7])
        );
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
