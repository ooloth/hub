//! Whether a review of my pull request is waiting on me, settled against my last push.
//!
//! A review waits on me when someone else submitted it after my last push and I have not replied
//! since. A reply is any review of mine, a reply in a review thread among them (GitHub stores each
//! thread reply as a review), or a top-level comment of mine.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Whether someone who submitted a review is a person or a bot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewerKind {
    /// A person's account.
    Person,
    /// A bot's account, such as Copilot's reviewer.
    Bot,
}

/// Who submitted a review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reviewer {
    /// I did.
    Me,
    /// Someone else did.
    Other {
        /// Their GitHub login.
        login: String,
        /// Whether they are a person or a bot.
        kind: ReviewerKind,
    },
}

/// What a submitted review concluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// It approved the pull request.
    Approved,
    /// It asked for changes.
    ChangesRequested,
    /// It commented without a verdict. A reply in a review thread is one of these.
    Commented,
    /// It was dismissed.
    Dismissed,
}

/// One submitted review of a pull request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Review {
    /// Who submitted it.
    pub by: Reviewer,
    /// What it concluded.
    pub verdict: Verdict,
    /// How many inline comments it carries.
    pub inline_comments: u32,
    /// Whether it has a body that is not blank.
    pub has_body: bool,
    /// When it was submitted.
    pub submitted: DateTime<Utc>,
}

/// The newest reviews of my pull request and the top-level comments I left on it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReviewActivity {
    /// Submitted reviews, in any order.
    pub reviews: Vec<Review>,
    /// When I left each top-level comment, in any order.
    pub my_comments: Vec<DateTime<Utc>>,
}

/// A review from someone else that asks something of me.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feedback {
    by: String,
    at: DateTime<Utc>,
}

impl Feedback {
    /// The feedback `review` gives me, if it asks something of me.
    ///
    /// An approval, a dismissed review and my own reviews ask nothing. Nor does a review with no
    /// inline comments from a bot, such as Copilot's overview, or one with no body.
    #[must_use]
    pub fn from_review(review: &Review) -> Option<Self> {
        let Reviewer::Other { login, kind } = &review.by else {
            return None;
        };
        let asks = match review.verdict {
            Verdict::Approved | Verdict::Dismissed => false,
            Verdict::ChangesRequested => true,
            Verdict::Commented => {
                review.inline_comments > 0 || (*kind == ReviewerKind::Person && review.has_body)
            }
        };
        asks.then(|| Self {
            by: login.clone(),
            at: review.submitted,
        })
    }

    /// Who left it.
    #[must_use]
    pub fn by(&self) -> &str {
        &self.by
    }

    /// When they submitted it.
    #[must_use]
    pub const fn at(&self) -> DateTime<Utc> {
        self.at
    }

    /// Whether it still waits on me, given `push`, my last push to the branch, or `None` when
    /// that could not be read.
    ///
    /// # Panics
    /// When the answer contradicts the times it was worked out from, which only a bug here can
    /// cause.
    #[must_use]
    pub fn against(self, push: Option<&LastPush>) -> ReviewWaiting {
        let Some(push) = push else {
            return ReviewWaiting::PushUnknown(self);
        };
        let waiting = if self.at > push.at {
            ReviewWaiting::Waiting(self)
        } else {
            ReviewWaiting::Answered {
                feedback: self,
                by: Answer::Push(push.at),
            }
        };
        match &waiting {
            ReviewWaiting::Waiting(feedback) => assert!(
                feedback.at > push.at,
                "feedback at {} waits on me though I pushed at {}",
                feedback.at,
                push.at
            ),
            ReviewWaiting::Answered { feedback, .. } => assert!(
                push.at >= feedback.at,
                "a push at {} answers feedback from later, at {}",
                push.at,
                feedback.at
            ),
            ReviewWaiting::NoReview | ReviewWaiting::PushUnknown(_) => {
                unreachable!("a known push always settles feedback")
            }
        }
        waiting
    }
}

/// A commit, named by its full object id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitOid(String);

impl CommitOid {
    /// The commit `oid` names.
    ///
    /// # Panics
    /// When `oid` is empty.
    #[must_use]
    pub fn new(oid: impl Into<String>) -> Self {
        let oid = oid.into();
        assert!(!oid.is_empty(), "a commit id is not empty");
        Self(oid)
    }
}

impl std::fmt::Display for CommitOid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// My last push to a pull request's branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastPush {
    /// When it happened.
    pub at: DateTime<Utc>,
    /// The commit it left at the head of the branch.
    pub head: CommitOid,
}

/// The latest feedback on my pull request, and whether I replied to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LatestFeedback {
    /// Nobody else left a review that asks something of me.
    None,
    /// I replied after it.
    Replied {
        /// The feedback.
        feedback: Feedback,
        /// When I replied.
        at: DateTime<Utc>,
    },
    /// I have not replied since it.
    Unreplied(Feedback),
}

impl ReviewActivity {
    /// The latest review from someone else that asks something of me, and my latest reply when
    /// it came at or after it.
    ///
    /// # Panics
    /// When the answer contradicts the times it was worked out from, which only a bug here can
    /// cause.
    #[must_use]
    pub fn latest_feedback(&self) -> LatestFeedback {
        let Some(feedback) = self
            .reviews
            .iter()
            .filter_map(Feedback::from_review)
            .max_by_key(Feedback::at)
        else {
            return LatestFeedback::None;
        };
        let my_reviews = self
            .reviews
            .iter()
            .filter(|review| review.by == Reviewer::Me)
            .map(|review| review.submitted);
        let latest_reply = my_reviews.chain(self.my_comments.iter().copied()).max();
        let latest = match latest_reply {
            Some(at) if at >= feedback.at => LatestFeedback::Replied { feedback, at },
            _ => LatestFeedback::Unreplied(feedback),
        };
        match &latest {
            LatestFeedback::Replied { feedback, at } => assert!(
                *at >= feedback.at,
                "a reply at {at} answers feedback from later, at {}",
                feedback.at
            ),
            LatestFeedback::Unreplied(feedback) => assert!(
                latest_reply.is_none_or(|reply| reply < feedback.at),
                "feedback at {} is unreplied though I replied at {latest_reply:?}",
                feedback.at
            ),
            LatestFeedback::None => {}
        }
        latest
    }
}

impl LatestFeedback {
    /// What waits on me when no push needs to be read, or the feedback that needs my last push
    /// to settle it.
    ///
    /// # Errors
    /// The unreplied feedback, which [`Feedback::against`] settles once my last push is known.
    pub fn settle_without_push(self) -> Result<ReviewWaiting, Feedback> {
        match self {
            Self::None => Ok(ReviewWaiting::NoReview),
            Self::Replied { feedback, at } => Ok(ReviewWaiting::Answered {
                feedback,
                by: Answer::Reply(at),
            }),
            Self::Unreplied(feedback) => Err(feedback),
        }
    }
}

/// What answered a review.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Answer {
    /// I replied at this time.
    Reply(DateTime<Utc>),
    /// I pushed at this time.
    Push(DateTime<Utc>),
}

/// Whether a review of my pull request waits on me.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewWaiting {
    /// Nobody else left a review that asks something of me.
    #[default]
    NoReview,
    /// The latest such review was answered.
    Answered {
        /// The review.
        feedback: Feedback,
        /// What answered it.
        by: Answer,
    },
    /// The latest such review waits on me.
    Waiting(Feedback),
    /// The latest such review is unreplied, and my last push could not be read.
    PushUnknown(Feedback),
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    fn minute(m: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000 + m * 60, 0).unwrap()
    }

    fn person(login: &str) -> Reviewer {
        Reviewer::Other {
            login: login.to_string(),
            kind: ReviewerKind::Person,
        }
    }

    fn bot(login: &str) -> Reviewer {
        Reviewer::Other {
            login: login.to_string(),
            kind: ReviewerKind::Bot,
        }
    }

    fn review(by: Reviewer, verdict: Verdict, inline: u32, body: bool, at: i64) -> Review {
        Review {
            by,
            verdict,
            inline_comments: inline,
            has_body: body,
            submitted: minute(at),
        }
    }

    fn push(at: i64) -> LastPush {
        LastPush {
            at: minute(at),
            head: CommitOid::new("abc123"),
        }
    }

    fn settle(activity: &ReviewActivity, last_push: Option<&LastPush>) -> ReviewWaiting {
        match activity.latest_feedback().settle_without_push() {
            Ok(waiting) => waiting,
            Err(feedback) => feedback.against(last_push),
        }
    }

    fn activity(reviews: Vec<Review>, my_comments: &[i64]) -> ReviewActivity {
        ReviewActivity {
            reviews,
            my_comments: my_comments.iter().map(|m| minute(*m)).collect(),
        }
    }

    fn is_waiting(waiting: &ReviewWaiting) -> bool {
        matches!(waiting, ReviewWaiting::Waiting(_))
    }

    #[rstest]
    #[case::changes_requested(review(person("ada"), Verdict::ChangesRequested, 0, false, 10))]
    #[case::a_persons_body(review(person("ada"), Verdict::Commented, 0, true, 10))]
    #[case::a_persons_inline_comment(review(person("ada"), Verdict::Commented, 1, false, 10))]
    #[case::a_bots_inline_comment(review(bot("copilot"), Verdict::Commented, 2, true, 10))]
    fn a_review_that_asks_something_after_my_push_waits_on_me(#[case] review: Review) {
        let waiting = settle(&activity(vec![review], &[]), Some(&push(5)));

        assert!(is_waiting(&waiting), "{waiting:?}");
    }

    #[rstest]
    #[case::an_approval(review(person("ada"), Verdict::Approved, 3, true, 10))]
    #[case::a_dismissed_review(review(person("ada"), Verdict::Dismissed, 3, true, 10))]
    #[case::a_bots_overview(review(bot("copilot"), Verdict::Commented, 0, true, 10))]
    #[case::an_empty_comment(review(person("ada"), Verdict::Commented, 0, false, 10))]
    #[case::my_own_review(review(Reviewer::Me, Verdict::Commented, 3, true, 10))]
    fn a_review_that_asks_nothing_is_not_feedback(#[case] review: Review) {
        let waiting = settle(&activity(vec![review], &[]), Some(&push(5)));

        assert_eq!(waiting, ReviewWaiting::NoReview);
    }

    #[test]
    fn my_reply_in_its_thread_answers_it() {
        let reviews = vec![
            review(person("ada"), Verdict::Commented, 1, false, 10),
            review(Reviewer::Me, Verdict::Commented, 1, false, 12),
        ];

        let waiting = settle(&activity(reviews, &[]), Some(&push(5)));

        assert!(matches!(
            waiting,
            ReviewWaiting::Answered { by: Answer::Reply(at), .. } if at == minute(12)
        ));
    }

    #[test]
    fn my_top_level_comment_answers_it() {
        let reviews = vec![review(
            person("ada"),
            Verdict::ChangesRequested,
            0,
            true,
            10,
        )];

        let waiting = settle(&activity(reviews, &[11]), Some(&push(5)));

        assert!(matches!(
            waiting,
            ReviewWaiting::Answered { by: Answer::Reply(at), .. } if at == minute(11)
        ));
    }

    #[test]
    fn a_push_after_it_answers_it() {
        let reviews = vec![review(
            person("ada"),
            Verdict::ChangesRequested,
            0,
            true,
            10,
        )];

        let waiting = settle(&activity(reviews, &[]), Some(&push(15)));

        assert!(matches!(
            waiting,
            ReviewWaiting::Answered { by: Answer::Push(at), .. } if at == minute(15)
        ));
    }

    #[test]
    fn a_review_submitted_after_my_push_waits_whatever_commit_it_was_made_on() {
        // GitHub records a review against the commit the reviewer was looking at, so only the
        // times say whether it came after my push.
        let reviews = vec![review(person("ada"), Verdict::Commented, 4, true, 20)];

        let waiting = settle(&activity(reviews, &[]), Some(&push(15)));

        assert_eq!(
            waiting,
            ReviewWaiting::Waiting(Feedback {
                by: "ada".to_string(),
                at: minute(20)
            })
        );
    }

    #[test]
    fn an_unreplied_review_with_no_known_push_is_unknown() {
        let reviews = vec![review(
            person("ada"),
            Verdict::ChangesRequested,
            0,
            true,
            10,
        )];

        let waiting = settle(&activity(reviews, &[]), None);

        assert!(
            matches!(waiting, ReviewWaiting::PushUnknown(_)),
            "{waiting:?}"
        );
    }

    #[test]
    fn a_reply_or_push_needs_no_lookup_when_nothing_asks() {
        let latest = activity(vec![], &[3]).latest_feedback();

        assert_eq!(latest.settle_without_push(), Ok(ReviewWaiting::NoReview));
    }

    fn reviewer() -> impl Strategy<Value = Reviewer> {
        prop_oneof![
            Just(Reviewer::Me),
            Just(person("ada")),
            Just(person("bo")),
            Just(bot("copilot")),
        ]
    }

    fn verdict() -> impl Strategy<Value = Verdict> {
        prop_oneof![
            Just(Verdict::Approved),
            Just(Verdict::ChangesRequested),
            Just(Verdict::Commented),
            Just(Verdict::Dismissed),
        ]
    }

    /// Reviews within one hour, so times collide and interleave with replies and the push.
    fn reviews() -> impl Strategy<Value = Vec<Review>> {
        prop::collection::vec(
            (reviewer(), verdict(), 0..3_u32, any::<bool>(), 0..60_i64)
                .prop_map(|(by, verdict, inline, body, at)| review(by, verdict, inline, body, at)),
            0..8,
        )
    }

    /// Whether `review` asks something of me, written out case by case.
    fn asks(review: &Review) -> bool {
        match (&review.by, review.verdict) {
            (Reviewer::Me, _) | (_, Verdict::Approved | Verdict::Dismissed) => false,
            (_, Verdict::ChangesRequested) => true,
            (Reviewer::Other { kind, .. }, Verdict::Commented) => {
                review.inline_comments > 0 || (*kind == ReviewerKind::Person && review.has_body)
            }
        }
    }

    proptest! {
        #[test]
        fn a_review_waits_on_me_exactly_when_it_is_newer_than_my_last_push_and_every_reply(
            reviews in reviews(),
            my_comments in prop::collection::vec(0..60_i64, 0..3),
            pushed in 0..60_i64,
        ) {
            let activity = activity(reviews.clone(), &my_comments);
            let latest_ask = reviews.iter().filter(|r| asks(r)).map(|r| r.submitted).max();
            let latest_answer = reviews
                .iter()
                .filter(|r| r.by == Reviewer::Me)
                .map(|r| r.submitted)
                .chain(my_comments.iter().map(|m| minute(*m)))
                .chain(std::iter::once(minute(pushed)))
                .max()
                .unwrap();

            let waiting = settle(&activity, Some(&push(pushed)));

            let expected = latest_ask.is_some_and(|at| at > latest_answer);
            prop_assert_eq!(is_waiting(&waiting), expected, "{:?}", waiting);
            if let ReviewWaiting::Waiting(feedback) = &waiting {
                prop_assert_eq!(Some(feedback.at()), latest_ask);
                prop_assert!(reviews.iter().any(|r| asks(r)
                    && r.submitted == feedback.at()
                    && (r.by == person(feedback.by()) || r.by == bot(feedback.by()))));
            }
        }
    }
}
