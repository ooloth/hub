//! Obtaining the daemon's credentials, trying again inside the process when 1Password fails, and
//! the line each failed attempt leaves in the log.
use std::fmt;
use std::future::Future;
use std::time::Duration;

use chrono::Utc;
use domain::credential_attempt::CredentialAttempt;
use domain::credential_failure::{CredentialCause, CredentialFailure};
use domain::pass_failure::PassFailure;

use crate::pass::one_line;

/// The wait before the second attempt.
const FIRST_DELAY: Duration = Duration::from_secs(1);

/// The longest wait between attempts, so an outage that clears is noticed within a minute.
const LONGEST_DELAY: Duration = Duration::from_secs(60);

/// The waits between attempts: one second, doubling each time, never more than a minute. Endless,
/// since a daemon with nobody present has nothing better to do than wait for 1Password.
#[derive(Debug, Clone)]
pub(crate) struct Backoff {
    next: Duration,
}

impl Backoff {
    pub(crate) const fn new() -> Self {
        Self { next: FIRST_DELAY }
    }
}

impl Iterator for Backoff {
    type Item = Duration;

    fn next(&mut self) -> Option<Duration> {
        let wait = self.next;
        self.next = wait.saturating_mul(2).min(LONGEST_DELAY);
        assert!(
            wait >= FIRST_DELAY,
            "a wait of {wait:?} is shorter than a second"
        );
        assert!(
            wait <= LONGEST_DELAY,
            "a wait of {wait:?} is longer than a minute"
        );
        Some(wait)
    }
}

/// How long to wait after a failed attempt, if at all.
#[derive(Debug, Clone)]
pub(crate) enum Retries {
    /// One attempt only, for `--once`.
    Never,
    /// Until credentials arrive: soon after an attempt that ended because something was not
    /// answering, so a fresh prompt is on screen within a second or two, and on the backoff after
    /// a fast failure.
    UntilObtained(Backoff),
}

impl Retries {
    pub(crate) const fn until_obtained() -> Self {
        Self::UntilObtained(Backoff::new())
    }

    /// The wait after an attempt that failed with `cause` and took `took`, or `None` for no more
    /// attempts.
    pub(crate) fn after(&mut self, cause: CredentialCause, took: Duration) -> Option<Duration> {
        match (self, cause) {
            (Self::Never, _) => None,
            (
                Self::UntilObtained(_),
                CredentialCause::TouchIdPromptUnanswered
                | CredentialCause::MacosPromptWaitingForYou
                | CredentialCause::AppRunningNotAnswering,
            ) => Some(after_not_answering(took)),
            (Self::UntilObtained(backoff), _) => backoff.next(),
        }
    }
}

/// The wait after an attempt that ended because something was not answering and took `took`: the
/// rest of a minute, and at least a second.
///
/// `op` gives up on a Touch ID prompt after a minute and the deadline ends a silent `op` after 90
/// seconds, so this is usually a second and a fresh attempt, with its prompt, follows at once.
/// However soon an attempt ended, attempts still start no more than once a minute.
fn after_not_answering(took: Duration) -> Duration {
    let wait = LONGEST_DELAY.saturating_sub(took).max(FIRST_DELAY);
    assert!(
        wait >= FIRST_DELAY,
        "a wait of {wait:?} is shorter than a second"
    );
    assert!(
        wait <= LONGEST_DELAY,
        "a wait of {wait:?} is longer than a minute"
    );
    assert!(
        took + wait >= LONGEST_DELAY,
        "attempts {took:?} + {wait:?} apart start less than a minute apart"
    );
    wait
}

/// Calls `resolve` until it returns credentials, waiting as `retries` says between attempts, and
/// hands every failed attempt to `record` before waiting.
///
/// Returns `None` once `retries` says to stop, after the last attempt has been recorded. With
/// [`Retries::UntilObtained`] it returns only with credentials.
pub(crate) async fn obtain<T, F, Fut>(
    mut resolve: F,
    mut retries: Retries,
    mut record: impl FnMut(&CredentialAttempt),
) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, CredentialFailure>>,
{
    // Bounded by `retries`: endless only when the caller asks to wait for 1Password forever.
    for attempt in 1_u32.. {
        let started_at = Utc::now();
        let started = std::time::Instant::now();
        let failure = match resolve().await {
            Ok(credentials) => return Some(credentials),
            Err(failure) => failure,
        };
        let duration = started.elapsed();
        let retry_in = retries.after(failure.cause(), duration);
        record(&CredentialAttempt {
            started_at,
            duration,
            pid: std::process::id(),
            attempt,
            failure,
            retry_in,
        });
        tokio::time::sleep(retry_in?).await;
    }
    None
}

/// One failed attempt, as it is logged: what failed, and whether that reached the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttemptReport {
    pub(crate) profile: String,
    pub(crate) attempt: CredentialAttempt,
    /// Why the health record could not be written, when it could not.
    pub(crate) record_error: Option<PassFailure>,
}

impl fmt::Display for AttemptReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let attempt = &self.attempt;
        write!(
            f,
            "hub-daemon credentials at={} pid={} profile={} attempt={} cause={} duration_ms={}",
            attempt
                .started_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            attempt.pid,
            self.profile,
            attempt.attempt,
            attempt.failure.cause().as_str(),
            attempt.duration.as_millis(),
        )?;
        if let Some(retry_in) = attempt.retry_in {
            write!(f, " retry_in={}", humantime::format_duration(retry_in))?;
        }
        write!(
            f,
            " error=\"{}\"",
            one_line(attempt.failure.reason().as_str())
        )?;
        self.record_error.as_ref().map_or(Ok(()), |error| {
            write!(
                f,
                " recorded=no record_error=\"{}\"",
                one_line(error.reason().as_str())
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use chrono::TimeZone;
    use domain::known_secrets::KnownSecrets;
    use domain::screen_at_deadline::ScreenAtDeadline;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    fn no_secrets() -> KnownSecrets {
        KnownSecrets::new(Vec::new())
    }

    fn failure(message: &str) -> CredentialFailure {
        CredentialFailure::new(&anyhow::anyhow!("{message}"), &no_secrets())
    }

    #[test]
    fn the_waits_start_at_a_second_and_double_up_to_a_minute() {
        let waits: Vec<u64> = Backoff::new().take(9).map(|wait| wait.as_secs()).collect();

        assert_eq!(waits, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
    }

    proptest! {
        #[test]
        fn every_wait_is_between_a_second_and_a_minute_and_never_shorter_than_the_last(
            count in 1_usize..500
        ) {
            let waits: Vec<Duration> = Backoff::new().take(count).collect();

            prop_assert!(waits.iter().all(|wait| (FIRST_DELAY..=LONGEST_DELAY).contains(wait)));
            prop_assert!(waits.windows(2).all(|pair| pair[0] <= pair[1]));
        }
    }

    const UNANSWERED: &str = "error initializing client: authorization timeout";

    #[rstest]
    #[case::at_op_s_limit(60_090, 1)]
    #[case::early(5_000, 55)]
    #[case::instantly(0, 60)]
    #[case::past_a_minute(90_000, 1)]
    fn after_an_unanswered_prompt_the_wait_is_the_rest_of_a_minute(
        #[case] took_ms: u64,
        #[case] wait_secs: u64,
    ) {
        let mut retries = Retries::until_obtained();

        let wait = retries.after(
            CredentialCause::TouchIdPromptUnanswered,
            Duration::from_millis(took_ms),
        );

        assert_eq!(wait.map(|wait| wait.as_secs()), Some(wait_secs));
    }

    proptest! {
        #[test]
        fn an_unanswered_prompt_never_starts_attempts_less_than_a_minute_apart(
            took_ms in 0_u64..200_000
        ) {
            let took = Duration::from_millis(took_ms);

            let wait = after_not_answering(took);

            prop_assert!((FIRST_DELAY..=LONGEST_DELAY).contains(&wait), "{wait:?}");
            prop_assert!(took + wait >= LONGEST_DELAY, "{took:?} + {wait:?}");
        }
    }

    #[rstest]
    #[case::unreachable(CredentialCause::Unreachable)]
    #[case::app_not_running(CredentialCause::AppNotRunning)]
    #[case::other(CredentialCause::Other)]
    fn every_fast_failure_waits_on_the_backoff(#[case] cause: CredentialCause) {
        let mut retries = Retries::until_obtained();

        let waits: Vec<u64> = (0..4)
            .filter_map(|_| retries.after(cause, Duration::from_secs(90)))
            .map(|wait| wait.as_secs())
            .collect();

        assert_eq!(waits, vec![1, 2, 4, 8]);
    }

    #[rstest]
    #[case::touch_id(CredentialCause::TouchIdPromptUnanswered)]
    #[case::macos_prompt(CredentialCause::MacosPromptWaitingForYou)]
    #[case::app_running(CredentialCause::AppRunningNotAnswering)]
    fn every_cause_that_was_not_answering_gets_the_short_wait_every_time(
        #[case] cause: CredentialCause,
    ) {
        let mut retries = Retries::until_obtained();

        // A deadline attempt lasts 90 s, so the short wait is a second each time, where the backoff
        // would have doubled.
        let waits: Vec<u64> = (0..3)
            .filter_map(|_| retries.after(cause, Duration::from_secs(90)))
            .map(|wait| wait.as_secs())
            .collect();

        assert_eq!(waits, vec![1, 1, 1]);
    }

    #[test]
    fn never_means_no_wait_whatever_the_cause() {
        assert_eq!(
            Retries::Never.after(CredentialCause::TouchIdPromptUnanswered, Duration::ZERO),
            None
        );
    }

    #[tokio::test(start_paused = true)]
    async fn unanswered_prompts_then_approval_give_credentials_with_each_wait_recorded() {
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&recorded);
        let resolve = failing_then(vec![failure(UNANSWERED), failure(UNANSWERED)], "secrets");

        let obtained = obtain(resolve, Retries::until_obtained(), move |attempt| {
            log.borrow_mut()
                .push((attempt.failure.cause(), attempt.retry_in));
        })
        .await;

        assert_eq!(obtained, Some("secrets"));
        // Each stubbed attempt takes almost no time, so each wait is nearly the whole minute,
        // where the backoff would have waited 1 then 2 seconds.
        let recorded = recorded.borrow();
        assert_eq!(recorded.len(), 2);
        for (cause, wait) in recorded.iter() {
            assert_eq!(*cause, CredentialCause::TouchIdPromptUnanswered);
            let wait = wait.expect("retried");
            assert!(
                wait > Duration::from_secs(59) && wait <= LONGEST_DELAY,
                "{wait:?}"
            );
        }
    }

    /// A resolver that fails with `failures` in order, then returns `credentials`.
    fn failing_then(
        failures: Vec<CredentialFailure>,
        credentials: &'static str,
    ) -> impl FnMut() -> std::future::Ready<Result<&'static str, CredentialFailure>> {
        let mut failures = failures.into_iter();
        move || std::future::ready(failures.next().map_or(Ok(credentials), Err))
    }

    #[tokio::test(start_paused = true)]
    async fn credentials_arrive_after_every_failed_attempt_is_recorded_in_order() {
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&recorded);
        let resolve = failing_then(
            vec![failure("dial tcp"), failure("desktop"), failure("dial tcp")],
            "secrets",
        );

        let obtained = obtain(resolve, Retries::until_obtained(), move |attempt| {
            log.borrow_mut().push((attempt.attempt, attempt.retry_in));
        })
        .await;

        assert_eq!(obtained, Some("secrets"));
        assert_eq!(
            *recorded.borrow(),
            vec![
                (1, Some(Duration::from_secs(1))),
                (2, Some(Duration::from_secs(2))),
                (3, Some(Duration::from_secs(4))),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn each_retry_waits_its_delay_first() {
        let started = tokio::time::Instant::now();
        let resolve = failing_then(vec![failure("dial tcp"), failure("dial tcp")], "secrets");

        let _ = obtain(resolve, Retries::until_obtained(), |_| {}).await;

        assert_eq!(started.elapsed(), Duration::from_secs(3));
    }

    #[tokio::test(start_paused = true)]
    async fn with_no_retries_one_failure_is_recorded_and_nothing_is_tried_again() {
        let calls = Rc::new(RefCell::new(0));
        let counted = Rc::clone(&calls);
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&recorded);
        let resolve = move || {
            *counted.borrow_mut() += 1;
            std::future::ready(Err::<(), _>(failure("dial tcp")))
        };

        let obtained = obtain(resolve, Retries::Never, move |attempt| {
            log.borrow_mut().push((attempt.attempt, attempt.retry_in));
        })
        .await;

        assert_eq!(obtained, None);
        assert_eq!(*calls.borrow(), 1);
        assert_eq!(*recorded.borrow(), vec![(1, None)]);
    }

    fn report(retry_in: Option<Duration>, record_error: Option<PassFailure>) -> AttemptReport {
        AttemptReport {
            profile: "default".to_string(),
            attempt: CredentialAttempt {
                started_at: Utc.with_ymd_and_hms(2026, 10, 12, 8, 59, 3).unwrap(),
                duration: Duration::from_millis(812),
                pid: 4242,
                attempt: 3,
                failure: failure(
                    "failed to resolve credential from 1Password: could not read secret \
                     'op://Scripts/GitHub/token': 1Password CLI couldn't connect to the \
                     1Password desktop app.",
                ),
                retry_in,
            },
            record_error,
        }
    }

    #[rstest]
    #[case::retrying(
        report(Some(Duration::from_secs(4)), None),
        "hub-daemon credentials at=2026-10-12T08:59:03Z pid=4242 profile=default attempt=3 \
         cause=app_not_running duration_ms=812 retry_in=4s error=\"failed to resolve credential \
         from 1Password: could not read secret 'op://Scripts/GitHub/token': 1Password CLI \
         couldn't connect to the 1Password desktop app.\""
    )]
    #[case::giving_up(
        report(None, None),
        "hub-daemon credentials at=2026-10-12T08:59:03Z pid=4242 profile=default attempt=3 \
         cause=app_not_running duration_ms=812 error=\"failed to resolve credential from \
         1Password: could not read secret 'op://Scripts/GitHub/token': 1Password CLI couldn't \
         connect to the 1Password desktop app.\""
    )]
    #[case::unrecorded(
        report(
            Some(Duration::from_secs(60)),
            Some(PassFailure::new(&anyhow::anyhow!("database is locked"), &no_secrets())),
        ),
        "hub-daemon credentials at=2026-10-12T08:59:03Z pid=4242 profile=default attempt=3 \
         cause=app_not_running duration_ms=812 retry_in=1m error=\"failed to resolve credential \
         from 1Password: could not read secret 'op://Scripts/GitHub/token': 1Password CLI \
         couldn't connect to the 1Password desktop app.\" recorded=no \
         record_error=\"database is locked\""
    )]
    fn a_failed_attempt_logs_when_who_which_why_and_what_next(
        #[case] report: AttemptReport,
        #[case] line: &str,
    ) {
        assert_eq!(report.to_string(), line);
    }

    #[test]
    fn the_line_names_who_was_not_answering_in_the_words_the_record_uses() {
        let mut prompt = report(Some(Duration::from_secs(1)), None);
        prompt.attempt.failure = failure(UNANSWERED);
        let mut alert = report(Some(Duration::from_secs(1)), None);
        alert.attempt.failure = CredentialFailure::no_answer(
            "op://Scripts/GitHub/token",
            Duration::from_secs(90),
            &ScreenAtDeadline::MacosAlert { windows: 1 },
            &no_secrets(),
        );

        assert!(
            prompt
                .to_string()
                .contains(" cause=touch_id_prompt_unanswered "),
            "{prompt}"
        );
        assert!(
            alert
                .to_string()
                .contains(" cause=macos_prompt_waiting_for_you "),
            "{alert}"
        );
        assert!(
            alert.to_string().ends_with(
                "error=\"1Password did not answer within 90s reading op://Scripts/GitHub/token; 1 \
                 macOS alert window(s) from UserNotificationCenter on screen\""
            ),
            "{alert}"
        );
    }
}
