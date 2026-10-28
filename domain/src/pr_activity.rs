//! What one refresh read about a pull request's reviewers, threads and verdicts.
//!
//! Who is asked to review it, who spoke last in each open thread, and what each reviewer
//! concluded. These are facts, read whole on every refresh. Nothing here compares them with a push or with an
//! earlier refresh (Decision 030).
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Someone who took part in a pull request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Participant {
    /// Me, or an agent acting under my account, which hub cannot tell apart.
    Me,
    /// A person's account, with its GitHub login.
    Person(String),
    /// A bot's account, such as Copilot's reviewer, with its GitHub login.
    Bot(String),
}

impl Participant {
    /// Their GitHub login, unless it is me.
    #[must_use]
    pub fn other_login(&self) -> Option<&str> {
        match self {
            Self::Me => None,
            Self::Person(login) | Self::Bot(login) => Some(login),
        }
    }
}

/// Someone a review is still requested from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Requested {
    /// A person, by login.
    Person(String),
    /// A bot, such as Copilot, by login.
    Bot(String),
    /// A team, or any other kind of reviewer GitHub can be asked for, by its kind, since a team's
    /// name is not readable with hub's token.
    Team(String),
}

impl Requested {
    /// Whether this request is still waiting on `login`.
    fn is_for(&self, login: &str) -> bool {
        match self {
            Self::Person(requested) | Self::Bot(requested) => requested == login,
            Self::Team(_) => false,
        }
    }
}

/// An unresolved review thread, read from its newest comments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenThread {
    last: Participant,
    at: DateTime<Utc>,
    people: Vec<String>,
}

impl OpenThread {
    /// The thread whose newest comments are `comments`, oldest first, or `None` when there are
    /// none to read.
    #[must_use]
    pub fn from_comments(comments: &[(Participant, DateTime<Utc>)]) -> Option<Self> {
        let (last, at) = comments.last()?.clone();
        let mut people: Vec<String> = comments
            .iter()
            .filter_map(|(participant, _)| match participant {
                Participant::Person(login) => Some(login.clone()),
                Participant::Me | Participant::Bot(_) => None,
            })
            .collect();
        people.sort();
        people.dedup();
        Some(Self { last, at, people })
    }

    /// Who wrote its newest comment.
    #[must_use]
    pub const fn last(&self) -> &Participant {
        &self.last
    }

    /// The feedback it holds for me: its newest comment, when someone else wrote it.
    fn feedback(&self) -> Option<Feedback> {
        self.last.other_login().map(|login| Feedback {
            by: login.to_string(),
            at: self.at,
        })
    }

    /// Whether it waits on someone else: I wrote its newest comment, and a person other than me
    /// took part, so a reply from them is the next move. A thread with only me and bots in it
    /// waits on nobody, since a bot does not answer.
    #[must_use]
    pub fn waits_on_someone_else(&self) -> bool {
        self.last == Participant::Me && !self.people.is_empty()
    }
}

/// A verdict a reviewer can leave.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Opinion {
    /// They approved it.
    Approved,
    /// They asked for changes.
    ChangesRequested,
}

/// A reviewer's latest verdict on a pull request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestVerdict {
    /// Who gave it.
    pub by: Participant,
    /// What it was.
    pub opinion: Opinion,
    /// When they gave it.
    pub at: DateTime<Utc>,
}

/// A review a person left with something written in its body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewBody {
    /// Their login.
    pub by: String,
    /// When they submitted it.
    pub at: DateTime<Utc>,
}

/// Something someone else left that asks something of me.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feedback {
    /// Who left it.
    pub by: String,
    /// When they left it.
    pub at: DateTime<Utc>,
}

/// What one refresh read about a pull request's review activity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrActivity {
    /// Everyone a review is still requested from.
    pub requests: Vec<Requested>,
    /// Its unresolved review threads.
    pub open_threads: Vec<OpenThread>,
    /// Each reviewer's latest approval or request for changes.
    pub verdicts: Vec<LatestVerdict>,
    /// Reviews people left with a body.
    pub bodies: Vec<ReviewBody>,
    /// When I last did anything on it: a review, a reply in a thread, or a top-level comment.
    pub my_latest: Option<DateTime<Utc>>,
}

impl PrActivity {
    /// The newest feedback waiting on me, if any is: a thread whose newest comment is someone
    /// else's, changes requested by a reviewer who has not been asked again, or a person's review
    /// body newer than anything of mine.
    #[must_use]
    pub fn feedback(&self) -> Option<Feedback> {
        self.feedback_items()
            .into_iter()
            .max_by_key(|feedback| feedback.at)
    }

    /// Whether `login` left feedback that waits on me.
    #[must_use]
    pub fn has_feedback_from(&self, login: &str) -> bool {
        self.feedback_items()
            .iter()
            .any(|feedback| feedback.by == login)
    }

    /// Whether the next move is someone else's: a review is still requested, or a thread waits on
    /// a person's reply to me.
    #[must_use]
    pub fn waits_on_someone_else(&self) -> bool {
        !self.requests.is_empty()
            || self
                .open_threads
                .iter()
                .any(OpenThread::waits_on_someone_else)
    }

    /// How many reviewers' latest verdict is `opinion`.
    ///
    /// # Panics
    /// When more reviewers than `u32::MAX` gave it, which a query reading at most 100 verdicts
    /// cannot produce.
    #[must_use]
    pub fn count(&self, opinion: Opinion) -> u32 {
        let count = self
            .verdicts
            .iter()
            .filter(|verdict| verdict.opinion == opinion)
            .count();
        let Ok(count) = u32::try_from(count) else {
            unreachable!("{count} reviewers gave one verdict, more than any query reads");
        };
        count
    }

    fn feedback_items(&self) -> Vec<Feedback> {
        let threads = self.open_threads.iter().filter_map(OpenThread::feedback);
        let changes = self.verdicts.iter().filter_map(|verdict| {
            let login = verdict.by.other_login()?;
            let asked_again = self.requests.iter().any(|request| request.is_for(login));
            (verdict.opinion == Opinion::ChangesRequested && !asked_again).then(|| Feedback {
                by: login.to_string(),
                at: verdict.at,
            })
        });
        let bodies = self
            .bodies
            .iter()
            .filter(|body| self.my_latest.is_none_or(|mine| body.at > mine))
            .map(|body| Feedback {
                by: body.by.clone(),
                at: body.at,
            });
        threads.chain(changes).chain(bodies).collect()
    }
}
