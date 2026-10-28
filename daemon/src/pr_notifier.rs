//! Keeping a notification showing for each pull request that is my turn, beside the passes.
//!
//! Each pass hands over the turns it saw and carries on: posting can take up to ten seconds a card,
//! and a pass must not wait on it. The notifier works through the newest turns it has been given,
//! so turns from a pass it had no time for are skipped rather than queued.
use std::fmt;
use std::future::Future;

use anyhow::Result;
use chrono::{DateTime, Utc};
use domain::notified::{ChangeOutcome, Notified};
use domain::pr_group::{group_for, ShownCards};
use domain::pr_id::PrId;
use domain::pr_turn::TurnCard;
use domain::profile::Profile;
use domain::turns_seen::{TurnCoverage, TurnsSeen};
use tokio::sync::watch;

use crate::pass::one_line;

/// Posts and removes pull request cards to match the turns it is given.
pub(crate) struct PrNotifier<P, R> {
    profile: Profile,
    notified: Notified,
    post: P,
    remove: R,
}

impl<P, PostFut, R, RemoveFut> PrNotifier<P, R>
where
    P: FnMut(String, TurnCard) -> PostFut,
    PostFut: Future<Output = Result<()>>,
    R: FnMut(String) -> RemoveFut,
    RemoveFut: Future<Output = Result<()>>,
{
    /// A notifier for `profile` that starts from `notified`, posting with `post` and removing with
    /// `remove`, each given the card's group.
    pub(crate) const fn new(profile: Profile, notified: Notified, post: P, remove: R) -> Self {
        Self {
            profile,
            notified,
            post,
            remove,
        }
    }

    /// Posts a card for each new turn in `seen`, posts again the card of each turn whose reason
    /// changed, and removes the card of each turn that ended.
    pub(crate) async fn apply(&mut self, seen: &TurnsSeen) -> NotifierReport {
        let changes = self.notified.changes_for(seen);
        let mut outcomes =
            Vec::with_capacity(changes.post.len() + changes.repost.len() + changes.remove.len());
        let posts = changes.post.into_iter().map(|turn| (turn, false));
        let reposts = changes.repost.into_iter().map(|turn| (turn, true));
        // A repost goes to the card's own group, so it replaces the card rather than adding one.
        for (turn, again) in posts.chain(reposts) {
            let group = group_for(self.profile, &turn.pr);
            outcomes.push(match (self.post)(group, turn.card()).await {
                Ok(()) if again => ChangeOutcome::Reposted {
                    pr: turn.pr,
                    reason: turn.reason,
                },
                Ok(()) => ChangeOutcome::Posted {
                    pr: turn.pr,
                    reason: turn.reason,
                },
                Err(error) => ChangeOutcome::PostFailed {
                    pr: turn.pr,
                    reason: format!("{error:#}"),
                },
            });
        }
        for pr in changes.remove {
            let group = group_for(self.profile, &pr);
            outcomes.push(match (self.remove)(group).await {
                Ok(()) => ChangeOutcome::Removed(pr),
                Err(error) => ChangeOutcome::RemoveFailed {
                    pr,
                    reason: format!("{error:#}"),
                },
            });
        }
        self.notified = std::mem::take(&mut self.notified).record(&outcomes);
        NotifierReport {
            at: Utc::now(),
            pid: std::process::id(),
            profile: self.profile,
            turns: seen.turns().len(),
            coverage: seen.coverage(),
            outcomes,
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

/// What one round of posting and removing did, as it is logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotifierReport {
    pub(crate) at: DateTime<Utc>,
    pub(crate) pid: u32,
    pub(crate) profile: Profile,
    /// How many pull requests were my turn.
    pub(crate) turns: usize,
    /// Whether removals were possible: with a pull request source missing, none are made.
    pub(crate) coverage: TurnCoverage,
    pub(crate) outcomes: Vec<ChangeOutcome>,
}

impl fmt::Display for NotifierReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let coverage = match self.coverage {
            TurnCoverage::Complete => "complete",
            TurnCoverage::Incomplete => "incomplete",
        };
        write!(
            f,
            "hub-daemon notifications at={} pid={} profile={} turns={} coverage={coverage}",
            self.at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            self.pid,
            self.profile,
            self.turns,
        )?;
        let prs = |wanted: fn(&ChangeOutcome) -> Option<&PrId>| -> String {
            self.outcomes
                .iter()
                .filter_map(wanted)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        };
        let posted = prs(|outcome| match outcome {
            ChangeOutcome::Posted { pr, .. } => Some(pr),
            _ => None,
        });
        let reposted = prs(|outcome| match outcome {
            ChangeOutcome::Reposted { pr, .. } => Some(pr),
            _ => None,
        });
        let removed = prs(|outcome| match outcome {
            ChangeOutcome::Removed(pr) => Some(pr),
            _ => None,
        });
        write!(
            f,
            " posted=\"{posted}\" reposted=\"{reposted}\" removed=\"{removed}\""
        )?;
        let failures: Vec<String> = self
            .outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                ChangeOutcome::PostFailed { pr, reason } => Some(format!("post {pr}: {reason}")),
                ChangeOutcome::RemoveFailed { pr, reason } => {
                    Some(format!("remove {pr}: {reason}"))
                }
                ChangeOutcome::Posted { .. }
                | ChangeOutcome::Reposted { .. }
                | ChangeOutcome::Removed(_) => None,
            })
            .collect();
        if failures.is_empty() {
            Ok(())
        } else {
            write!(f, " failed=\"{}\"", one_line(&failures.join("; ")))
        }
    }
}

/// The cards found showing at startup, as it is logged: these are the ones a pass may remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CardsFound {
    pub(crate) at: DateTime<Utc>,
    pub(crate) pid: u32,
    pub(crate) profile: Profile,
    /// What was found, or why listing failed.
    pub(crate) found: Result<ShownCards, String>,
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
            Ok(shown) => {
                let prs: Vec<String> = shown.prs.iter().map(ToString::to_string).collect();
                write!(f, " prs=\"{}\"", prs.join(" "))?;
                if shown.unreadable.is_empty() {
                    Ok(())
                } else {
                    write!(
                        f,
                        " unreadable=\"{}\"",
                        one_line(&shown.unreadable.join(" "))
                    )
                }
            }
            Err(error) => write!(f, " error=\"{}\"", one_line(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    use chrono::TimeZone;
    use domain::pr_turn::TurnReason;
    use domain::{MergeReadiness, PrKind, PullRequest, RepoSlug, Urgency};

    use super::*;

    fn pr(number: u64) -> PullRequest {
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

    fn seen(numbers: &[u64]) -> TurnsSeen {
        let prs: Vec<PullRequest> = numbers.iter().map(|n| pr(*n)).collect();
        TurnsSeen::from_prs(&prs, TurnCoverage::Complete)
    }

    fn id(number: u64) -> PrId {
        PrId {
            repo: RepoSlug::new("ooloth", "hub"),
            number,
        }
    }

    /// Every group posted to and removed, in order.
    #[derive(Default)]
    struct Calls {
        posted: Vec<String>,
        removed: Vec<String>,
    }

    #[tokio::test]
    async fn cards_follow_the_turns_from_pass_to_pass() {
        let calls = Rc::new(RefCell::new(Calls::default()));
        let (posts, removes) = (Rc::clone(&calls), Rc::clone(&calls));
        let mut notifier = PrNotifier::new(
            Profile::Dev,
            Notified::default(),
            move |group, _card| {
                posts.borrow_mut().posted.push(group);
                std::future::ready(Ok(()))
            },
            move |group| {
                removes.borrow_mut().removed.push(group);
                std::future::ready(Ok(()))
            },
        );

        let _ = notifier.apply(&seen(&[1, 2])).await;
        let _ = notifier.apply(&seen(&[2, 3])).await;

        let calls = calls.borrow();
        assert_eq!(
            calls.posted,
            vec![
                "hub-dev-pr-ooloth/hub#1",
                "hub-dev-pr-ooloth/hub#2",
                "hub-dev-pr-ooloth/hub#3"
            ]
        );
        assert_eq!(calls.removed, vec!["hub-dev-pr-ooloth/hub#1"]);
    }

    #[tokio::test]
    async fn a_card_found_showing_at_startup_is_removed_once_its_turn_has_ended() {
        let removed = Rc::new(RefCell::new(Vec::new()));
        let removes = Rc::clone(&removed);
        let mut notifier = PrNotifier::new(
            Profile::Default,
            Notified::from_showing([id(5)]),
            |_, _| std::future::ready(Ok(())),
            move |group| {
                removes.borrow_mut().push(group);
                std::future::ready(Ok(()))
            },
        );

        let _ = notifier.apply(&seen(&[])).await;

        assert_eq!(*removed.borrow(), vec!["hub-default-pr-ooloth/hub#5"]);
    }

    #[tokio::test]
    async fn a_failed_post_is_reported_and_tried_again() {
        let attempts = Rc::new(RefCell::new(0));
        let counted = Rc::clone(&attempts);
        let mut notifier = PrNotifier::new(
            Profile::Dev,
            Notified::default(),
            move |_, _| {
                *counted.borrow_mut() += 1;
                std::future::ready(Err(anyhow::anyhow!("posting the card took too long")))
            },
            |_| std::future::ready(Ok(())),
        );

        let report = notifier.apply(&seen(&[1])).await;
        let _ = notifier.apply(&seen(&[1])).await;

        assert_eq!(*attempts.borrow(), 2);
        assert!(
            report
                .to_string()
                .ends_with("failed=\"post ooloth/hub#1: posting the card took too long\""),
            "{report}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_stuck_notifier_holds_up_no_pass_and_then_works_from_the_newest_turns() {
        let posted = Rc::new(RefCell::new(Vec::new()));
        let posts = Rc::clone(&posted);
        let notifier = PrNotifier::new(
            Profile::Dev,
            Notified::default(),
            move |group, _card| {
                let posts = Rc::clone(&posts);
                async move {
                    // Each post hangs as long as terminal-notifier may before it is given up on.
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    posts.borrow_mut().push(group);
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
                send.send(Some(seen(&[1]))).unwrap();
                tokio::task::yield_now().await;
                // Two passes while the first post is still hanging: sending returns at once.
                send.send(Some(seen(&[2]))).unwrap();
                send.send(Some(seen(&[3]))).unwrap();
                tokio::time::sleep(Duration::from_secs(60)).await;
            })
            .await;

        assert_eq!(
            *posted.borrow(),
            vec!["hub-dev-pr-ooloth/hub#1", "hub-dev-pr-ooloth/hub#3"]
        );
    }

    /// My pull request `number`, approved with nothing blocking the merge, and a thread whose
    /// newest comment is Ada's waiting on me when `reviewed`.
    fn mine(number: u64, reviewed: bool) -> PullRequest {
        use domain::pr_activity::{OpenThread, Participant, PrActivity};
        let thread = OpenThread::from_comments(&[(
            Participant::Person("ada".to_string()),
            chrono::DateTime::UNIX_EPOCH,
        )])
        .unwrap();
        PullRequest {
            kind: PrKind::Mine,
            draft: false,
            review_decision: Some(domain::ReviewDecision::Approved),
            merge_readiness: MergeReadiness::Ready,
            activity: PrActivity {
                open_threads: if reviewed { vec![thread] } else { vec![] },
                ..PrActivity::default()
            },
            ..pr(number)
        }
    }

    #[tokio::test]
    async fn a_card_is_posted_again_in_its_group_when_its_reason_changes() {
        let cards: Rc<RefCell<Vec<(String, String)>>> = Rc::default();
        let posts = Rc::clone(&cards);
        let mut notifier = PrNotifier::new(
            Profile::Dev,
            Notified::default(),
            move |group, card: TurnCard| {
                posts.borrow_mut().push((group, card.title));
                std::future::ready(Ok(()))
            },
            |_group| std::future::ready(Ok(())),
        );

        let _ = notifier
            .apply(&TurnsSeen::from_prs(
                &[mine(7, false)],
                TurnCoverage::Complete,
            ))
            .await;
        let report = notifier
            .apply(&TurnsSeen::from_prs(
                &[mine(7, true)],
                TurnCoverage::Complete,
            ))
            .await;
        let unchanged = notifier
            .apply(&TurnsSeen::from_prs(
                &[mine(7, true)],
                TurnCoverage::Complete,
            ))
            .await;

        assert_eq!(
            *cards.borrow(),
            vec![
                (
                    "hub-dev-pr-ooloth/hub#7".to_string(),
                    "PR ready to merge".to_string()
                ),
                (
                    "hub-dev-pr-ooloth/hub#7".to_string(),
                    "PR feedback".to_string()
                ),
            ]
        );
        assert!(
            report.to_string().contains("reposted=\"ooloth/hub#7\""),
            "{report}"
        );
        assert!(unchanged.outcomes.is_empty(), "{unchanged}");
    }

    #[test]
    fn a_round_is_one_line_naming_what_changed() {
        let report = NotifierReport {
            at: Utc.with_ymd_and_hms(2026, 10, 17, 23, 15, 55).unwrap(),
            pid: 4242,
            profile: Profile::Default,
            turns: 3,
            coverage: TurnCoverage::Incomplete,
            outcomes: vec![
                ChangeOutcome::Posted {
                    pr: id(1),
                    reason: TurnReason::ReviewRequested,
                },
                ChangeOutcome::Posted {
                    pr: id(2),
                    reason: TurnReason::ReviewRequested,
                },
                ChangeOutcome::Reposted {
                    pr: id(5),
                    reason: TurnReason::Feedback {
                        by: "ada".to_string(),
                    },
                },
                ChangeOutcome::Removed(id(3)),
                ChangeOutcome::RemoveFailed {
                    pr: id(4),
                    reason: "terminal-notifier exited 1:\nno".to_string(),
                },
            ],
        };

        assert_eq!(
            report.to_string(),
            "hub-daemon notifications at=2026-10-17T23:15:55Z pid=4242 profile=default turns=3 \
             coverage=incomplete posted=\"ooloth/hub#1 ooloth/hub#2\" reposted=\"ooloth/hub#5\" \
             removed=\"ooloth/hub#3\" \
             failed=\"remove ooloth/hub#4: terminal-notifier exited 1: no\""
        );
    }

    #[test]
    fn cards_found_at_startup_are_one_line() {
        let found = CardsFound {
            at: Utc.with_ymd_and_hms(2026, 10, 17, 23, 15, 55).unwrap(),
            pid: 4242,
            profile: Profile::Dev,
            found: Ok(ShownCards {
                prs: vec![id(1), id(2)],
                unreadable: vec!["hub-dev-pr-junk".to_string()],
            }),
        };

        assert_eq!(
            found.to_string(),
            "hub-daemon notifications_found at=2026-10-17T23:15:55Z pid=4242 profile=dev \
             prs=\"ooloth/hub#1 ooloth/hub#2\" unreadable=\"hub-dev-pr-junk\""
        );
    }
}
