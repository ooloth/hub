//! Keeping the card about pull requests waiting on me up to date, beside the passes.
//!
//! Each pass hands over the turns it saw and carries on: posting can take up to ten seconds, and a
//! pass must not wait on it. The notifier works through the newest turns it has been given, so
//! turns from a pass it had no time for are skipped rather than queued.
use std::fmt;
use std::future::Future;

use anyhow::Result;
use chrono::{DateTime, Utc};
use domain::pr_group::waiting_group;
use domain::pr_id::PrId;
use domain::profile::Profile;
use domain::turns_seen::{TurnCoverage, TurnsSeen};
use domain::waiting_card::{CardChange, WaitingCard};
use domain::waiting_on_me::WaitingOnMe;
use tokio::sync::watch;

use crate::pass::one_line;

/// Posts, removes or leaves the card to match the pull requests waiting on me, and removes the
/// per-pull-request cards an earlier build left showing.
pub(crate) struct PrNotifier<P, R> {
    profile: Profile,
    waiting: WaitingOnMe,
    leftovers: Vec<String>,
    post: P,
    remove: R,
}

impl<P, PostFut, R, RemoveFut> PrNotifier<P, R>
where
    P: FnMut(String, WaitingCard) -> PostFut,
    PostFut: Future<Output = Result<()>>,
    R: FnMut(String) -> RemoveFut,
    RemoveFut: Future<Output = Result<()>>,
{
    /// A notifier for `profile` that has `leftovers` to remove, posting with `post` and removing
    /// with `remove`, each given a group.
    pub(crate) fn new(profile: Profile, leftovers: Vec<String>, post: P, remove: R) -> Self {
        Self {
            profile,
            waiting: WaitingOnMe::default(),
            leftovers,
            post,
            remove,
        }
    }

    /// Counts the turns in `seen`, then posts, removes or leaves the card to match, and tries to
    /// remove each leftover card not yet removed.
    pub(crate) async fn apply(&mut self, seen: &TurnsSeen) -> NotifierReport {
        self.waiting = std::mem::take(&mut self.waiting).after(seen);
        let group = waiting_group(self.profile);
        let card = match CardChange::after(&self.waiting, seen) {
            CardChange::Show(card) => match (self.post)(group, card).await {
                Ok(()) => CardOutcome::Posted,
                Err(error) => CardOutcome::PostFailed(format!("{error:#}")),
            },
            CardChange::Remove => match (self.remove)(group).await {
                Ok(()) => CardOutcome::Removed,
                Err(error) => CardOutcome::RemoveFailed(format!("{error:#}")),
            },
            CardChange::Leave => CardOutcome::Left,
        };
        let mut leftovers_removed = Vec::new();
        let mut leftovers_failed = Vec::new();
        for leftover in std::mem::take(&mut self.leftovers) {
            match (self.remove)(leftover.clone()).await {
                Ok(()) => leftovers_removed.push(leftover),
                Err(error) => leftovers_failed.push((leftover, format!("{error:#}"))),
            }
        }
        self.leftovers = leftovers_failed
            .iter()
            .map(|(group, _)| group.clone())
            .collect();
        NotifierReport {
            at: Utc::now(),
            pid: std::process::id(),
            profile: self.profile,
            turns: seen.turns().len(),
            coverage: seen.coverage(),
            counted: self.waiting.prs().cloned().collect(),
            card,
            leftovers_removed,
            leftovers_failed,
        }
    }

    /// Applies the newest turns each time new ones arrive, until no pass is left to send any.
    pub(crate) async fn run(mut self, mut turns: watch::Receiver<Option<TurnsSeen>>) {
        while turns.changed().await.is_ok() {
            let newest = turns.borrow_and_update().clone();
            if let Some(seen) = newest {
                println!("{}", self.apply(&seen).await);
            }
        }
    }
}

/// What happened to the card after one refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CardOutcome {
    /// It was posted, so it shows again.
    Posted,
    /// It could not be posted, so the next refresh posts it again.
    PostFailed(String),
    /// Nothing is waiting on me, and it was removed.
    Removed,
    /// It could not be removed, so the next refresh tries again.
    RemoveFailed(String),
    /// Nothing is counted, but a source did not answer, so it was left as it was.
    Left,
}

/// What one round of keeping the card up to date did, as it is logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotifierReport {
    pub(crate) at: DateTime<Utc>,
    pub(crate) pid: u32,
    pub(crate) profile: Profile,
    /// How many pull requests this refresh found waiting on me.
    pub(crate) turns: usize,
    /// Whether every pull request source answered.
    pub(crate) coverage: TurnCoverage,
    /// The pull requests the card counted, including ones carried from earlier refreshes.
    pub(crate) counted: Vec<PrId>,
    pub(crate) card: CardOutcome,
    /// Leftover cards removed this round.
    pub(crate) leftovers_removed: Vec<String>,
    /// Leftover cards that could not be removed, and why.
    pub(crate) leftovers_failed: Vec<(String, String)>,
}

impl fmt::Display for NotifierReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let coverage = match self.coverage {
            TurnCoverage::Complete => "complete",
            TurnCoverage::Incomplete => "incomplete",
        };
        let counted: Vec<String> = self.counted.iter().map(ToString::to_string).collect();
        let card = match self.card {
            CardOutcome::Posted => "posted",
            CardOutcome::PostFailed(_) => "post_failed",
            CardOutcome::Removed => "removed",
            CardOutcome::RemoveFailed(_) => "remove_failed",
            CardOutcome::Left => "left",
        };
        write!(
            f,
            "hub-daemon notifications at={} pid={} profile={} turns={} coverage={coverage} \
             counted=\"{}\" card={card}",
            self.at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            self.pid,
            self.profile,
            self.turns,
            counted.join(" "),
        )?;
        if !self.leftovers_removed.is_empty() {
            write!(
                f,
                " leftovers_removed=\"{}\"",
                self.leftovers_removed.join(" ")
            )?;
        }
        let card_failure = match &self.card {
            CardOutcome::PostFailed(reason) => Some(format!("post: {reason}")),
            CardOutcome::RemoveFailed(reason) => Some(format!("remove: {reason}")),
            CardOutcome::Posted | CardOutcome::Removed | CardOutcome::Left => None,
        };
        let failures: Vec<String> = card_failure
            .into_iter()
            .chain(
                self.leftovers_failed
                    .iter()
                    .map(|(group, reason)| format!("remove {group}: {reason}")),
            )
            .collect();
        if failures.is_empty() {
            Ok(())
        } else {
            write!(f, " failed=\"{}\"", one_line(&failures.join("; ")))
        }
    }
}

/// The leftover per-pull-request cards found showing at startup, as it is logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CardsFound {
    pub(crate) at: DateTime<Utc>,
    pub(crate) pid: u32,
    pub(crate) profile: Profile,
    /// The leftover groups found, or why listing failed.
    pub(crate) found: Result<Vec<String>, String>,
}

impl fmt::Display for CardsFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "hub-daemon notifications_found at={} pid={} profile={}",
            self.at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            self.pid,
            self.profile,
        )?;
        match &self.found {
            Ok(leftovers) => write!(f, " leftovers=\"{}\"", one_line(&leftovers.join(" "))),
            Err(error) => write!(f, " error=\"{}\"", one_line(error)),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    use chrono::TimeZone;
    use domain::{MergeReadiness, PrKind, PullRequest, RepoSlug, Urgency};

    use super::*;

    /// A pull request whose review is requested from me.
    pub(crate) fn pr(number: u64) -> PullRequest {
        PullRequest {
            number,
            title: format!("title {number}"),
            repo: RepoSlug::new("ooloth", "hub"),
            url: format!("https://github.com/ooloth/hub/pull/{number}"),
            age: chrono::Duration::zero(),
            urgency: Urgency::High,
            kind: PrKind::ToReview,
            draft: false,
            author: "someone".to_string(),
            review_decision: None,
            comment_count: 0,
            head_branch: "h".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_readiness: MergeReadiness::NotYetKnown,
            activity: domain::pr_activity::PrActivity::default(),
        }
    }

    fn seen(numbers: &[u64], coverage: TurnCoverage) -> TurnsSeen {
        let prs: Vec<PullRequest> = numbers.iter().map(|n| pr(*n)).collect();
        TurnsSeen::from_prs(&prs, coverage)
    }

    fn complete(numbers: &[u64]) -> TurnsSeen {
        seen(numbers, TurnCoverage::Complete)
    }

    fn id(number: u64) -> PrId {
        PrId {
            repo: RepoSlug::new("ooloth", "hub"),
            number,
        }
    }

    /// Every card posted, as its group and title, and every group removed, in order.
    #[derive(Default)]
    struct Calls {
        posted: Vec<(String, String)>,
        removed: Vec<String>,
    }

    /// A notifier for the dev profile with `leftovers`, whose posts and removals are recorded in
    /// the calls returned, and whose removals of the groups in `failing` fail.
    fn recording(
        leftovers: &[&str],
        failing: &'static [&'static str],
    ) -> (
        Rc<RefCell<Calls>>,
        PrNotifier<
            impl FnMut(String, WaitingCard) -> std::future::Ready<Result<()>>,
            impl FnMut(String) -> std::future::Ready<Result<()>>,
        >,
    ) {
        let calls = Rc::new(RefCell::new(Calls::default()));
        let (posts, removes) = (Rc::clone(&calls), Rc::clone(&calls));
        let notifier = PrNotifier::new(
            Profile::Dev,
            leftovers.iter().map(ToString::to_string).collect(),
            move |group, card: WaitingCard| {
                posts.borrow_mut().posted.push((group, card.title()));
                std::future::ready(Ok(()))
            },
            move |group: String| {
                let fails = failing.contains(&group.as_str());
                removes.borrow_mut().removed.push(group);
                std::future::ready(if fails {
                    Err(anyhow::anyhow!("removing the card took too long"))
                } else {
                    Ok(())
                })
            },
        );
        (calls, notifier)
    }

    #[tokio::test]
    async fn every_refresh_with_a_turn_posts_the_card_to_the_same_group() {
        let (calls, mut notifier) = recording(&[], &[]);

        let _ = notifier.apply(&complete(&[1, 2])).await;
        let _ = notifier.apply(&complete(&[1, 2])).await;
        let _ = notifier.apply(&complete(&[2])).await;

        assert_eq!(
            calls.borrow().posted,
            vec![
                (
                    "hub-dev-turns".to_string(),
                    "2 PRs are waiting for you".to_string()
                ),
                (
                    "hub-dev-turns".to_string(),
                    "2 PRs are waiting for you".to_string()
                ),
                (
                    "hub-dev-turns".to_string(),
                    "1 PR is waiting for you".to_string()
                ),
            ]
        );
    }

    #[tokio::test]
    async fn a_complete_refresh_with_no_turn_removes_the_card() {
        let (calls, mut notifier) = recording(&[], &[]);

        let _ = notifier.apply(&complete(&[1])).await;
        let report = notifier.apply(&complete(&[])).await;

        assert_eq!(calls.borrow().removed, vec!["hub-dev-turns"]);
        assert_eq!(report.card, CardOutcome::Removed);
    }

    #[tokio::test]
    async fn an_incomplete_refresh_keeps_counting_what_it_could_not_check() {
        let (calls, mut notifier) = recording(&[], &[]);

        let _ = notifier.apply(&complete(&[1])).await;
        let report = notifier.apply(&seen(&[], TurnCoverage::Incomplete)).await;

        assert_eq!(report.counted, vec![id(1)]);
        assert_eq!(report.card, CardOutcome::Posted);
        assert!(calls.borrow().removed.is_empty());
    }

    #[tokio::test]
    async fn leftover_cards_are_removed_on_the_first_refresh_and_a_failed_one_again_on_the_next() {
        let (calls, mut notifier) = recording(
            &["hub-dev-pr-ooloth/hub#1", "hub-dev-pr-ooloth/hub#2"],
            &["hub-dev-pr-ooloth/hub#2"],
        );

        let first = notifier.apply(&complete(&[3])).await;
        let second = notifier.apply(&complete(&[3])).await;
        let third = notifier.apply(&complete(&[3])).await;

        assert_eq!(first.leftovers_removed, vec!["hub-dev-pr-ooloth/hub#1"]);
        assert_eq!(
            calls.borrow().removed,
            vec![
                "hub-dev-pr-ooloth/hub#1",
                "hub-dev-pr-ooloth/hub#2",
                "hub-dev-pr-ooloth/hub#2",
                "hub-dev-pr-ooloth/hub#2",
            ]
        );
        assert_eq!(
            second.leftovers_failed,
            vec![(
                "hub-dev-pr-ooloth/hub#2".to_string(),
                "removing the card took too long".to_string()
            )]
        );
        assert!(third.leftovers_removed.is_empty(), "{third}");
    }

    #[tokio::test]
    async fn a_failed_post_is_reported_and_made_again_on_the_next_refresh() {
        let attempts = Rc::new(RefCell::new(0));
        let counted = Rc::clone(&attempts);
        let mut notifier = PrNotifier::new(
            Profile::Dev,
            vec![],
            move |_, _| {
                *counted.borrow_mut() += 1;
                std::future::ready(Err(anyhow::anyhow!("posting the card took too long")))
            },
            |_| std::future::ready(Ok(())),
        );

        let report = notifier.apply(&complete(&[1])).await;
        let _ = notifier.apply(&complete(&[1])).await;

        assert_eq!(*attempts.borrow(), 2);
        assert!(
            report
                .to_string()
                .ends_with("card=post_failed failed=\"post: posting the card took too long\""),
            "{report}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_stuck_notifier_holds_up_no_pass_and_then_works_from_the_newest_turns() {
        let posted = Rc::new(RefCell::new(Vec::new()));
        let posts = Rc::clone(&posted);
        let notifier = PrNotifier::new(
            Profile::Dev,
            vec![],
            move |_group, card: WaitingCard| {
                let posts = Rc::clone(&posts);
                async move {
                    // Each post hangs as long as terminal-notifier may before it is given up on.
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    posts.borrow_mut().push(card.title());
                    Ok(())
                }
            },
            |_| std::future::ready(Ok(())),
        );
        let (send, receive) = watch::channel(None);
        let local = tokio::task::LocalSet::new();
        let _running = local.spawn_local(notifier.run(receive));

        local
            .run_until(async {
                send.send(Some(complete(&[1]))).unwrap();
                tokio::task::yield_now().await;
                // Two passes while the first post is still hanging: sending returns at once.
                send.send(Some(complete(&[1, 2]))).unwrap();
                send.send(Some(complete(&[1, 2, 3]))).unwrap();
                tokio::time::sleep(Duration::from_secs(60)).await;
            })
            .await;

        assert_eq!(
            *posted.borrow(),
            vec!["1 PR is waiting for you", "3 PRs are waiting for you"]
        );
    }

    #[test]
    fn a_round_is_one_line_naming_what_was_counted_and_what_happened_to_the_card() {
        let report = NotifierReport {
            at: Utc.with_ymd_and_hms(2026, 10, 28, 23, 15, 55).unwrap(),
            pid: 4242,
            profile: Profile::Default,
            turns: 1,
            coverage: TurnCoverage::Incomplete,
            counted: vec![id(1), id(5)],
            card: CardOutcome::Posted,
            leftovers_removed: vec!["hub-default-pr-ooloth/hub#3".to_string()],
            leftovers_failed: vec![(
                "hub-default-pr-ooloth/hub#4".to_string(),
                "terminal-notifier exited 1:\nno".to_string(),
            )],
        };

        assert_eq!(
            report.to_string(),
            "hub-daemon notifications at=2026-10-28T23:15:55Z pid=4242 profile=default turns=1 \
             coverage=incomplete counted=\"ooloth/hub#1 ooloth/hub#5\" card=posted \
             leftovers_removed=\"hub-default-pr-ooloth/hub#3\" \
             failed=\"remove hub-default-pr-ooloth/hub#4: terminal-notifier exited 1: no\""
        );
    }

    #[test]
    fn a_round_that_left_the_card_says_so() {
        let report = NotifierReport {
            at: Utc.with_ymd_and_hms(2026, 10, 28, 23, 15, 55).unwrap(),
            pid: 4242,
            profile: Profile::Dev,
            turns: 0,
            coverage: TurnCoverage::Incomplete,
            counted: vec![],
            card: CardOutcome::Left,
            leftovers_removed: vec![],
            leftovers_failed: vec![],
        };

        assert_eq!(
            report.to_string(),
            "hub-daemon notifications at=2026-10-28T23:15:55Z pid=4242 profile=dev turns=0 \
             coverage=incomplete counted=\"\" card=left"
        );
    }

    #[test]
    fn leftovers_found_at_startup_are_one_line() {
        let found = CardsFound {
            at: Utc.with_ymd_and_hms(2026, 10, 28, 23, 15, 55).unwrap(),
            pid: 4242,
            profile: Profile::Dev,
            found: Ok(vec![
                "hub-dev-pr-ooloth/hub#1".to_string(),
                "hub-dev-pr-ooloth/hub#2".to_string(),
            ]),
        };

        assert_eq!(
            found.to_string(),
            "hub-daemon notifications_found at=2026-10-28T23:15:55Z pid=4242 profile=dev \
             leftovers=\"hub-dev-pr-ooloth/hub#1 hub-dev-pr-ooloth/hub#2\""
        );
    }
}
