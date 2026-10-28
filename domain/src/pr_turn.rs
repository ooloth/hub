//! Whether a pull request is waiting on me, and the card that says so.
//!
//! The answer is read from what one refresh saw of the pull request (Decision 030). On a pull
//! request I opened, the move is mine unless it is visibly waiting on someone else, and anything
//! waiting on me overrides that. The rules run in a fixed order, so the reason named is the most
//! useful next move.
use crate::pr::{
    CiStatus, MergeBlocker, MergeReadiness, PrKind, PullRequest, ReviewDecision, ReviewStatus,
};
use crate::pr_id::PrId;
use crate::untrusted::UntrustedText;

/// Why a pull request is waiting on me.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnReason {
    /// Someone asked for my review.
    ReviewRequested,
    /// It is mine, and someone left something that asks something of me.
    Feedback {
        /// Who left the newest of it.
        by: String,
    },
    /// It is mine, and a check failed.
    ChecksFailing,
    /// It is mine, and still a draft.
    Draft,
    /// It is mine, approved, and nothing but running checks or an out-of-date branch stops the
    /// merge.
    ReadyToMerge,
    /// It is mine, approved, and its branch conflicts with its base.
    Conflicts,
    /// It is mine, nobody else is asked to review it, and nothing waits on anyone else.
    NoReviewer,
}

impl TurnReason {
    /// The words that say why, wherever a turn is shown.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ReviewRequested => "review requested",
            Self::Feedback { .. } => "feedback",
            Self::ChecksFailing => "checks failing",
            Self::Draft => "draft",
            Self::ReadyToMerge => "ready to merge",
            Self::Conflicts => "conflicts",
            Self::NoReviewer => "no reviewer",
        }
    }
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
    /// It might be, and this refresh could not tell: it is mine and approved, and GitHub has not
    /// worked out whether anything stops the merge.
    Undecided,
}

impl TurnReading {
    /// Whether `pr` is waiting on me, and why.
    ///
    /// # Panics
    /// When the reading contradicts the facts it was read from, which only a bug here can cause.
    #[must_use]
    pub fn of(pr: &PullRequest) -> Self {
        let reading = match pr.kind {
            PrKind::ToReview if !pr.draft => Ok(TurnReason::ReviewRequested),
            PrKind::ToReview | PrKind::External => Err(NoTurn::NotMyTurn),
            PrKind::Mine => my_turn(pr),
        };
        let reading = match reading {
            Ok(reason) => Self::Turn(Turn {
                reason,
                pr: PrId::of(pr),
                author: pr.author.clone(),
                title: UntrustedText::new(pr.title.clone()),
                url: pr.url.clone(),
            }),
            Err(NoTurn::NotMyTurn) => Self::NotMyTurn,
            Err(NoTurn::Undecided) => Self::Undecided,
        };
        assert_agrees_with_facts(&reading, pr);
        reading
    }
}

/// Why a pull request is not my turn, or might not be.
enum NoTurn {
    /// It is not.
    NotMyTurn,
    /// It is mine and approved, and GitHub has not worked out whether anything stops the merge.
    Undecided,
}

/// Why my own pull request is my turn, in the order of the most useful next move, or why it is
/// not.
fn my_turn(pr: &PullRequest) -> Result<TurnReason, NoTurn> {
    if let Some(feedback) = pr.activity.feedback() {
        return Ok(TurnReason::Feedback { by: feedback.by });
    }
    if pr.ci_status == Some(CiStatus::Failure) {
        return Ok(TurnReason::ChecksFailing);
    }
    if pr.draft {
        return Ok(TurnReason::Draft);
    }
    if is_approved(pr) {
        let checks_running = pr.ci_status == Some(CiStatus::Pending);
        match pr.merge_readiness {
            MergeReadiness::Ready | MergeReadiness::Blocked(MergeBlocker::Behind) => {
                return Ok(TurnReason::ReadyToMerge);
            }
            MergeReadiness::Blocked(MergeBlocker::Blocked) if checks_running => {
                return Ok(TurnReason::ReadyToMerge);
            }
            MergeReadiness::Blocked(MergeBlocker::Conflict) => return Ok(TurnReason::Conflicts),
            MergeReadiness::NotYetKnown => return Err(NoTurn::Undecided),
            // Blocked by something other than running checks, so read as not approved.
            MergeReadiness::Blocked(MergeBlocker::Blocked) => {}
        }
    }
    if pr.activity.waits_on_someone_else() {
        return Err(NoTurn::NotMyTurn);
    }
    Ok(TurnReason::NoReviewer)
}

/// Whether `pr` is approved. GitHub's review decision settles it when branch protection gives
/// one, and each reviewer's latest verdict otherwise.
fn is_approved(pr: &PullRequest) -> bool {
    match pr.review_decision {
        Some(ReviewDecision::Approved) => true,
        Some(ReviewDecision::ChangesRequested | ReviewDecision::ReviewRequired) => false,
        None => matches!(pr.review_status(), ReviewStatus::Approved(_)),
    }
}

/// Halts when `reading` contradicts the facts of `pr` it was read from.
fn assert_agrees_with_facts(reading: &TurnReading, pr: &PullRequest) {
    match reading {
        TurnReading::Turn(turn) => {
            if pr.draft {
                assert_eq!(
                    pr.kind,
                    PrKind::Mine,
                    "someone else's draft is never my turn"
                );
            }
            match &turn.reason {
                TurnReason::ReviewRequested => {
                    assert_eq!(pr.kind, PrKind::ToReview, "a review request asks for me");
                }
                TurnReason::Feedback { by } => {
                    assert_eq!(pr.kind, PrKind::Mine, "feedback is on my pull request");
                    assert!(
                        pr.activity.has_feedback_from(by),
                        "feedback named {by}, who left none waiting on me"
                    );
                }
                TurnReason::Draft => {
                    assert_eq!(pr.kind, PrKind::Mine, "a draft turn is my own draft");
                    assert!(pr.draft, "a draft turn is a draft");
                }
                TurnReason::NoReviewer => {
                    assert_eq!(pr.kind, PrKind::Mine, "no reviewer is on my pull request");
                    assert!(
                        pr.activity.requests.is_empty(),
                        "no reviewer while a review is requested"
                    );
                    assert!(
                        !pr.activity
                            .open_threads
                            .iter()
                            .any(crate::pr_activity::OpenThread::waits_on_someone_else),
                        "no reviewer while a thread waits on someone else"
                    );
                }
                TurnReason::ChecksFailing | TurnReason::ReadyToMerge | TurnReason::Conflicts => {
                    assert_eq!(
                        pr.kind,
                        PrKind::Mine,
                        "only my own pull request is mine to fix or merge"
                    );
                }
            }
        }
        TurnReading::Undecided => {
            assert_eq!(
                pr.kind,
                PrKind::Mine,
                "only my own pull request is undecided"
            );
            assert!(
                is_approved(pr),
                "only an approved pull request is undecided"
            );
            assert_eq!(
                pr.merge_readiness,
                MergeReadiness::NotYetKnown,
                "only unknown mergeability leaves a pull request undecided"
            );
        }
        TurnReading::NotMyTurn => {}
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
        let subtitle = match &self.reason {
            TurnReason::ReviewRequested => format!("{repo_and_number} · @{}", self.author),
            TurnReason::Feedback { by } => format!("{repo_and_number} · @{by}"),
            TurnReason::ChecksFailing
            | TurnReason::Draft
            | TurnReason::ReadyToMerge
            | TurnReason::Conflicts
            | TurnReason::NoReviewer => repo_and_number,
        };
        TurnCard {
            title: format!("PR {}", self.reason.label()),
            subtitle,
            message: self.title.clone(),
            url: self.url.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::pr::RepoSlug;
    use crate::pr_activity::{
        LatestVerdict, OpenThread, Opinion, Participant, PrActivity, Requested, ReviewBody,
    };
    use crate::urgency::Urgency;

    fn minute(m: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000 + m * 60, 0).unwrap()
    }

    fn person(login: &str) -> Participant {
        Participant::Person(login.to_string())
    }

    fn bot(login: &str) -> Participant {
        Participant::Bot(login.to_string())
    }

    fn pr(kind: PrKind) -> PullRequest {
        PullRequest {
            number: 344,
            title: "[WIP] Notify when a pull request is my turn".to_string(),
            repo: RepoSlug::new("ooloth", "hub"),
            url: "https://github.com/ooloth/hub/pull/344".to_string(),
            age: Duration::zero(),
            urgency: Urgency::High,
            kind,
            draft: false,
            author: "someone".to_string(),
            review_decision: None,
            comment_count: 0,
            head_branch: "notify".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_readiness: MergeReadiness::Ready,
            activity: PrActivity::default(),
        }
    }

    fn mine() -> PullRequest {
        pr(PrKind::Mine)
    }

    fn thread(comments: &[(Participant, i64)]) -> OpenThread {
        let comments: Vec<_> = comments
            .iter()
            .map(|(by, at)| (by.clone(), minute(*at)))
            .collect();
        OpenThread::from_comments(&comments).unwrap()
    }

    fn verdict(by: Participant, opinion: Opinion, at: i64) -> LatestVerdict {
        LatestVerdict {
            by,
            opinion,
            at: minute(at),
        }
    }

    fn approved(mut pr: PullRequest) -> PullRequest {
        pr.activity
            .verdicts
            .push(verdict(person("ada"), Opinion::Approved, 1));
        pr
    }

    fn with_threads(mut pr: PullRequest, threads: Vec<OpenThread>) -> PullRequest {
        pr.activity.open_threads = threads;
        pr
    }

    fn requested(mut pr: PullRequest, request: Requested) -> PullRequest {
        pr.activity.requests.push(request);
        pr
    }

    fn reason(reading: &TurnReading) -> Option<TurnReason> {
        match reading {
            TurnReading::Turn(turn) => Some(turn.reason.clone()),
            TurnReading::NotMyTurn | TurnReading::Undecided => None,
        }
    }

    fn feedback_from(login: &str) -> Option<TurnReason> {
        Some(TurnReason::Feedback {
            by: login.to_string(),
        })
    }

    // ── someone else's pull requests ─────────────────────────────────────────

    #[test]
    fn a_review_request_is_my_turn() {
        assert_eq!(
            reason(&TurnReading::of(&pr(PrKind::ToReview))),
            Some(TurnReason::ReviewRequested)
        );
    }

    #[rstest]
    #[case::asking_for_my_review(PrKind::ToReview)]
    #[case::not_asking(PrKind::External)]
    fn someone_elses_draft_is_never_my_turn(#[case] kind: PrKind) {
        let mut draft = pr(kind);
        draft.draft = true;

        assert_eq!(TurnReading::of(&draft), TurnReading::NotMyTurn);
    }

    #[test]
    fn someone_elses_pull_request_that_does_not_ask_for_me_is_not_my_turn() {
        assert_eq!(
            TurnReading::of(&pr(PrKind::External)),
            TurnReading::NotMyTurn
        );
    }

    // ── feedback ──────────────────────────────────────────────────────────────

    #[rstest]
    #[case::a_person(person("ada"), "ada")]
    #[case::a_bot(bot("copilot-pull-request-reviewer"), "copilot-pull-request-reviewer")]
    fn a_thread_whose_newest_comment_is_someone_elses_is_my_turn_even_with_a_request_pending(
        #[case] last: Participant,
        #[case] login: &str,
    ) {
        let pr = requested(
            with_threads(mine(), vec![thread(&[(Participant::Me, 1), (last, 2)])]),
            Requested::Person("grace".to_string()),
        );

        assert_eq!(reason(&TurnReading::of(&pr)), feedback_from(login));
    }

    #[test]
    fn changes_requested_by_a_reviewer_not_asked_again_is_my_turn() {
        let mut pr = mine();
        pr.activity
            .verdicts
            .push(verdict(person("ada"), Opinion::ChangesRequested, 1));

        assert_eq!(reason(&TurnReading::of(&pr)), feedback_from("ada"));
    }

    #[test]
    fn asking_the_reviewer_who_requested_changes_again_hands_them_the_move() {
        let mut pr = requested(mine(), Requested::Person("ada".to_string()));
        pr.activity
            .verdicts
            .push(verdict(person("ada"), Opinion::ChangesRequested, 1));

        assert_eq!(TurnReading::of(&pr), TurnReading::NotMyTurn);
    }

    #[rstest]
    #[case::nothing_of_mine(None)]
    #[case::mine_before_it(Some(1))]
    fn a_persons_review_body_newer_than_anything_of_mine_is_my_turn(#[case] mine_at: Option<i64>) {
        let mut pr = requested(approved(mine()), Requested::Person("grace".to_string()));
        pr.activity.bodies.push(ReviewBody {
            by: "ada".to_string(),
            at: minute(2),
        });
        pr.activity.my_latest = mine_at.map(minute);

        assert_eq!(reason(&TurnReading::of(&pr)), feedback_from("ada"));
    }

    #[rstest]
    #[case::at_the_same_moment(2)]
    #[case::after_it(3)]
    fn anything_of_mine_after_a_review_body_answers_it(#[case] mine_at: i64) {
        let mut pr = requested(mine(), Requested::Person("grace".to_string()));
        pr.activity.bodies.push(ReviewBody {
            by: "ada".to_string(),
            at: minute(2),
        });
        pr.activity.my_latest = Some(minute(mine_at));

        assert_eq!(TurnReading::of(&pr), TurnReading::NotMyTurn);
    }

    #[test]
    fn the_newest_feedback_names_who_left_it() {
        let mut pr = with_threads(mine(), vec![thread(&[(person("ada"), 1)])]);
        pr.activity
            .verdicts
            .push(verdict(person("grace"), Opinion::ChangesRequested, 5));

        assert_eq!(reason(&TurnReading::of(&pr)), feedback_from("grace"));
    }

    // ── fixing ────────────────────────────────────────────────────────────────

    #[test]
    fn failing_checks_are_my_turn_ahead_of_draft_and_approval() {
        let mut pr = approved(mine());
        pr.draft = true;
        pr.ci_status = Some(CiStatus::Failure);

        assert_eq!(
            reason(&TurnReading::of(&pr)),
            Some(TurnReason::ChecksFailing)
        );
    }

    #[test]
    fn feedback_comes_before_failing_checks() {
        let mut pr = with_threads(mine(), vec![thread(&[(person("ada"), 1)])]);
        pr.ci_status = Some(CiStatus::Failure);

        assert_eq!(reason(&TurnReading::of(&pr)), feedback_from("ada"));
    }

    // ── drafts ────────────────────────────────────────────────────────────────

    #[test]
    fn my_draft_is_my_turn_even_while_a_review_is_requested() {
        let mut pr = requested(mine(), Requested::Person("ada".to_string()));
        pr.draft = true;

        assert_eq!(reason(&TurnReading::of(&pr)), Some(TurnReason::Draft));
    }

    #[test]
    fn feedback_on_my_draft_comes_before_its_being_a_draft() {
        let mut pr = with_threads(mine(), vec![thread(&[(person("ada"), 1)])]);
        pr.draft = true;

        assert_eq!(reason(&TurnReading::of(&pr)), feedback_from("ada"));
    }

    // ── approval ──────────────────────────────────────────────────────────────

    #[rstest]
    #[case::nothing_blocks_it(MergeReadiness::Ready, None, Some(TurnReason::ReadyToMerge))]
    #[case::only_behind(
        MergeReadiness::Blocked(MergeBlocker::Behind),
        None,
        Some(TurnReason::ReadyToMerge)
    )]
    #[case::only_running_checks(
        MergeReadiness::Blocked(MergeBlocker::Blocked),
        Some(CiStatus::Pending),
        Some(TurnReason::ReadyToMerge)
    )]
    #[case::conflicting(
        MergeReadiness::Blocked(MergeBlocker::Conflict),
        None,
        Some(TurnReason::Conflicts)
    )]
    fn my_approved_pull_request_is_my_turn_whatever_requests_are_pending(
        #[case] merge: MergeReadiness,
        #[case] checks: Option<CiStatus>,
        #[case] expected: Option<TurnReason>,
    ) {
        let mut pr = requested(approved(mine()), Requested::Person("grace".to_string()));
        pr.merge_readiness = merge;
        pr.ci_status = checks;

        assert_eq!(reason(&TurnReading::of(&pr)), expected);
    }

    #[test]
    fn an_approval_github_reports_only_as_a_decision_counts() {
        let mut pr = mine();
        pr.review_decision = Some(ReviewDecision::Approved);

        assert_eq!(
            reason(&TurnReading::of(&pr)),
            Some(TurnReason::ReadyToMerge)
        );
    }

    #[rstest]
    #[case::review_required(ReviewDecision::ReviewRequired)]
    #[case::changes_requested(ReviewDecision::ChangesRequested)]
    fn an_approval_branch_protection_does_not_accept_is_not_approved(
        #[case] decision: ReviewDecision,
    ) {
        let mut pr = requested(approved(mine()), Requested::Person("grace".to_string()));
        pr.review_decision = Some(decision);

        assert_eq!(TurnReading::of(&pr), TurnReading::NotMyTurn);
    }

    #[rstest]
    #[case::passing_checks(Some(CiStatus::Success))]
    #[case::no_checks(None)]
    fn my_approved_pull_request_blocked_by_anything_else_is_read_as_not_approved(
        #[case] checks: Option<CiStatus>,
    ) {
        let mut blocked = approved(mine());
        blocked.merge_readiness = MergeReadiness::Blocked(MergeBlocker::Blocked);
        blocked.ci_status = checks;
        let waiting = requested(blocked.clone(), Requested::Person("grace".to_string()));

        assert_eq!(
            reason(&TurnReading::of(&blocked)),
            Some(TurnReason::NoReviewer)
        );
        assert_eq!(TurnReading::of(&waiting), TurnReading::NotMyTurn);
    }

    #[test]
    fn my_approved_pull_request_is_undecided_until_github_works_out_the_merge() {
        let mut pr = approved(mine());
        pr.merge_readiness = MergeReadiness::NotYetKnown;

        assert_eq!(TurnReading::of(&pr), TurnReading::Undecided);
    }

    #[test]
    fn running_checks_neither_make_nor_end_a_turn() {
        let mut running = mine();
        running.ci_status = Some(CiStatus::Pending);
        let waiting = requested(running.clone(), Requested::Person("ada".to_string()));

        assert_eq!(
            reason(&TurnReading::of(&running)),
            Some(TurnReason::NoReviewer)
        );
        assert_eq!(TurnReading::of(&waiting), TurnReading::NotMyTurn);
    }

    // ── waiting on someone else ───────────────────────────────────────────────

    #[rstest]
    #[case::a_person(Requested::Person("ada".to_string()))]
    #[case::a_bot(Requested::Bot("Copilot".to_string()))]
    #[case::a_team(Requested::Team("reviewers".to_string()))]
    fn a_pending_review_request_hands_the_move_to_whoever_is_asked(#[case] request: Requested) {
        assert_eq!(
            TurnReading::of(&requested(mine(), request)),
            TurnReading::NotMyTurn
        );
    }

    #[test]
    fn a_persons_thread_i_answered_last_waits_on_them() {
        let pr = with_threads(
            mine(),
            vec![thread(&[(person("ada"), 1), (Participant::Me, 2)])],
        );

        assert_eq!(TurnReading::of(&pr), TurnReading::NotMyTurn);
    }

    #[test]
    fn a_thread_with_only_me_and_bots_in_it_waits_on_nobody() {
        let pr = with_threads(
            mine(),
            vec![thread(&[
                (bot("copilot-pull-request-reviewer"), 1),
                (Participant::Me, 2),
            ])],
        );

        assert_eq!(reason(&TurnReading::of(&pr)), Some(TurnReason::NoReviewer));
    }

    #[test]
    fn my_pull_request_nobody_is_asked_to_review_is_my_turn() {
        assert_eq!(
            reason(&TurnReading::of(&mine())),
            Some(TurnReason::NoReviewer)
        );
    }

    // ── what a turn carries ───────────────────────────────────────────────────

    #[test]
    fn a_turn_carries_the_pull_request_it_is_about() {
        let TurnReading::Turn(turn) = TurnReading::of(&pr(PrKind::ToReview)) else {
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
    fn a_feedback_card_names_who_left_it() {
        let card = turn(TurnReason::Feedback {
            by: "ada".to_string(),
        })
        .card();

        assert_eq!(card.title, "PR feedback");
        assert_eq!(card.subtitle, "hub #344 · @ada");
    }

    #[rstest]
    #[case(TurnReason::ChecksFailing)]
    #[case(TurnReason::Draft)]
    #[case(TurnReason::ReadyToMerge)]
    #[case(TurnReason::Conflicts)]
    #[case(TurnReason::NoReviewer)]
    fn a_card_about_my_own_pull_request_leaves_out_the_author_who_is_me(
        #[case] reason: TurnReason,
    ) {
        assert_eq!(turn(reason).card().subtitle, "hub #344");
    }

    #[rstest]
    #[case(TurnReason::ReviewRequested, "review requested")]
    #[case(TurnReason::Feedback { by: "ada".to_string() }, "feedback")]
    #[case(TurnReason::ChecksFailing, "checks failing")]
    #[case(TurnReason::Draft, "draft")]
    #[case(TurnReason::ReadyToMerge, "ready to merge")]
    #[case(TurnReason::Conflicts, "conflicts")]
    #[case(TurnReason::NoReviewer, "no reviewer")]
    fn a_cards_title_is_pr_followed_by_its_reasons_label(
        #[case] reason: TurnReason,
        #[case] label: &str,
    ) {
        assert_eq!(reason.label(), label);
        assert_eq!(turn(reason).card().title, format!("PR {label}"));
    }

    // ── generated ─────────────────────────────────────────────────────────────

    fn participant() -> impl Strategy<Value = Participant> {
        prop_oneof![
            Just(Participant::Me),
            prop::sample::select(vec!["ada", "grace"]).prop_map(|l| person(l)),
            Just(bot("copilot-pull-request-reviewer")),
        ]
    }

    fn open_thread() -> impl Strategy<Value = OpenThread> {
        prop::collection::vec((participant(), 0..20_i64), 1..5).prop_map(|mut comments| {
            comments.sort_by_key(|(_, at)| *at);
            thread(&comments)
        })
    }

    fn activity() -> impl Strategy<Value = PrActivity> {
        let request = prop_oneof![
            Just(Requested::Person("ada".to_string())),
            Just(Requested::Bot("copilot-pull-request-reviewer".to_string())),
            Just(Requested::Team("reviewers".to_string())),
        ];
        let opinion = prop_oneof![Just(Opinion::Approved), Just(Opinion::ChangesRequested)];
        let verdicts =
            prop::collection::vec((participant(), opinion, 0..20_i64), 0..3).prop_map(|verdicts| {
                let mut seen = Vec::new();
                verdicts
                    .into_iter()
                    .filter(|(by, _, _)| *by != Participant::Me)
                    .filter(|(by, _, _)| {
                        let fresh = !seen.contains(by);
                        seen.push(by.clone());
                        fresh
                    })
                    .map(|(by, opinion, at)| verdict(by, opinion, at))
                    .collect::<Vec<_>>()
            });
        let bodies = prop::collection::vec(
            (prop::sample::select(vec!["ada", "grace"]), 0..20_i64).prop_map(|(by, at)| {
                ReviewBody {
                    by: by.to_string(),
                    at: minute(at),
                }
            }),
            0..3,
        );
        (
            prop::collection::vec(request, 0..2),
            prop::collection::vec(open_thread(), 0..4),
            verdicts,
            bodies,
            prop::option::of(0..20_i64),
        )
            .prop_map(
                |(requests, open_threads, verdicts, bodies, my_latest)| PrActivity {
                    requests,
                    open_threads,
                    verdicts,
                    bodies,
                    my_latest: my_latest.map(minute),
                },
            )
    }

    fn any_pr() -> impl Strategy<Value = PullRequest> {
        let kind = prop_oneof![
            Just(PrKind::Mine),
            Just(PrKind::ToReview),
            Just(PrKind::External)
        ];
        let decision = prop::option::of(prop_oneof![
            Just(ReviewDecision::Approved),
            Just(ReviewDecision::ChangesRequested),
            Just(ReviewDecision::ReviewRequired),
        ]);
        let checks = prop::option::of(prop_oneof![
            Just(CiStatus::Success),
            Just(CiStatus::Failure),
            Just(CiStatus::Pending),
            Just(CiStatus::Neutral),
        ]);
        let merge = prop_oneof![
            Just(MergeReadiness::Ready),
            Just(MergeReadiness::NotYetKnown),
            Just(MergeReadiness::Blocked(MergeBlocker::Conflict)),
            Just(MergeReadiness::Blocked(MergeBlocker::Behind)),
            Just(MergeReadiness::Blocked(MergeBlocker::Blocked)),
        ];
        (kind, any::<bool>(), decision, checks, merge, activity()).prop_map(
            |(kind, draft, decision, checks, merge, activity)| {
                let mut pr = pr(kind);
                pr.draft = draft;
                pr.review_decision = decision;
                pr.ci_status = checks;
                pr.merge_readiness = merge;
                pr.activity = activity;
                pr
            },
        )
    }

    proptest! {
        /// `TurnReading::of` asserts that its answer agrees with the facts, so every generated pull
        /// request also exercises those assertions.
        #[test]
        fn undecided_arises_only_from_my_approved_pull_request_with_unknown_mergeability(
            pr in any_pr()
        ) {
            if TurnReading::of(&pr) == TurnReading::Undecided {
                prop_assert_eq!(pr.kind, PrKind::Mine);
                prop_assert_eq!(pr.merge_readiness, MergeReadiness::NotYetKnown);
            }
        }

        #[test]
        fn my_reply_in_one_thread_leaves_feedback_in_another_waiting_on_me(
            pr in any_pr(),
            at in 20..40_i64,
        ) {
            let mut pr = pr;
            pr.kind = PrKind::Mine;
            pr.activity.open_threads = vec![
                thread(&[(person("ada"), at)]),
                thread(&[(person("grace"), at)]),
            ];
            let mut replied = pr.clone();
            replied.activity.open_threads[0] = thread(&[(person("ada"), at), (Participant::Me, at + 1)]);

            let before = matches!(reason(&TurnReading::of(&pr)), Some(TurnReason::Feedback { .. }));
            let after = matches!(reason(&TurnReading::of(&replied)), Some(TurnReason::Feedback { .. }));
            prop_assert!(before);
            prop_assert!(after);
        }

        #[test]
        fn a_pull_request_read_back_from_the_cache_reads_the_same(pr in any_pr()) {
            let json = serde_json::to_string(&pr).unwrap();
            let cached: PullRequest = serde_json::from_str(&json).unwrap();

            prop_assert_eq!(TurnReading::of(&cached), TurnReading::of(&pr));
        }
    }
}
