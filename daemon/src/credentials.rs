//! Obtaining the daemon's credentials, trying again inside the process when 1Password fails, and
//! the line each failed attempt leaves in the log.
use std::fmt;
use std::future::Future;
use std::time::Duration;

use chrono::Utc;
use domain::credential_attempt::{AppOpening, CredentialAttempt, Notifying};
use domain::credential_failure::{CredentialCause, CredentialFailure};
use domain::credential_notification::Notification;
use domain::known_secrets::KnownSecrets;
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

/// How long after opening the 1Password app the daemon leaves it before opening it again.
const REOPEN_AFTER: Duration = Duration::from_secs(60);

/// Opens the 1Password app when an attempt found it not running, at most once per
/// [`REOPEN_AFTER`], so an app that will not stay open is not relaunched every second.
#[derive(Debug)]
pub(crate) struct AppOpener<O> {
    open: O,
    last_opened: Option<tokio::time::Instant>,
}

impl<O, Fut> AppOpener<O>
where
    O: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    /// `open` is what opens the app: `open -a 1Password` in the daemon, a stub in tests.
    pub(crate) const fn new(open: O) -> Self {
        Self {
            open,
            last_opened: None,
        }
    }

    /// Opens the app unless it was opened within the last [`REOPEN_AFTER`], and says how that
    /// went, or `None` when it was not due.
    async fn open_if_due(&mut self) -> Option<AppOpening> {
        let now = tokio::time::Instant::now();
        if self
            .last_opened
            .is_some_and(|last| now.duration_since(last) < REOPEN_AFTER)
        {
            return None;
        }
        self.last_opened = Some(now);
        Some(match (self.open)().await {
            Ok(()) => AppOpening::Opened,
            // Nothing has been resolved when the app is not running, so nothing to redact.
            Err(error) => AppOpening::failed(&error, &KnownSecrets::new(Vec::new())),
        })
    }
}

/// How long the daemon goes without credentials before it posts a notification.
const NOTIFY_AFTER: Duration = Duration::from_mins(10);

/// How long after one notification the daemon leaves it before posting another.
const NOTIFY_EVERY: Duration = Duration::from_hours(1);

/// Posts a notification once credentials have been missing for a while, and again every so often
/// while they stay missing.
#[derive(Debug)]
pub(crate) struct Notifier<N> {
    post: N,
    after: Duration,
    every: Duration,
    first_failure: Option<tokio::time::Instant>,
    last_posted: Option<tokio::time::Instant>,
}

impl<N, Fut> Notifier<N>
where
    N: FnMut(Notification) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    /// `post` is what posts: `osascript` in the daemon, a stub in tests. Notifies after
    /// [`NOTIFY_AFTER`] without credentials, then every [`NOTIFY_EVERY`].
    pub(crate) const fn new(post: N) -> Self {
        Self::with_timing(post, NOTIFY_AFTER, NOTIFY_EVERY)
    }

    pub(crate) const fn with_timing(post: N, after: Duration, every: Duration) -> Self {
        Self {
            post,
            after,
            every,
            first_failure: None,
            last_posted: None,
        }
    }

    /// Posts the notification for `cause` when credentials have been missing for `after` and none
    /// was posted in the last `every`, and says how that went, or `None` when none was due.
    async fn notify_if_due(&mut self, cause: CredentialCause) -> Option<Notifying> {
        let now = tokio::time::Instant::now();
        let first_failure = *self.first_failure.get_or_insert(now);
        let missing_long_enough = now.duration_since(first_failure) >= self.after;
        let posted_recently = self
            .last_posted
            .is_some_and(|last| now.duration_since(last) < self.every);
        if !missing_long_enough || posted_recently {
            return None;
        }
        self.last_posted = Some(now);
        Some(match (self.post)(Notification::for_cause(cause)).await {
            Ok(()) => Notifying::Sent,
            // Nothing has been resolved while credentials are missing, so nothing to redact.
            Err(error) => Notifying::failed(&error, &KnownSecrets::new(Vec::new())),
        })
    }
}

/// Calls `resolve` until it returns credentials, waiting as `retries` says between attempts, and
/// hands every failed attempt to `record` before waiting. An attempt that found the 1Password app
/// not running has `opener` open it first, and `notifier` posts when credentials have been missing
/// long enough. Time spent opening and posting comes off the wait, so the next attempt is not
/// pushed back.
///
/// Returns `None` once `retries` says to stop, after the last attempt has been recorded. With
/// [`Retries::UntilObtained`] it returns only with credentials.
pub(crate) async fn obtain<T, F, Fut, O, OpenFut, N, PostFut>(
    mut resolve: F,
    mut retries: Retries,
    opener: &mut AppOpener<O>,
    notifier: &mut Notifier<N>,
    mut record: impl FnMut(&CredentialAttempt),
) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, CredentialFailure>>,
    O: FnMut() -> OpenFut,
    OpenFut: Future<Output = anyhow::Result<()>>,
    N: FnMut(Notification) -> PostFut,
    PostFut: Future<Output = anyhow::Result<()>>,
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
        let acting = tokio::time::Instant::now();
        let opened_app = if failure.cause() == CredentialCause::AppNotRunning {
            opener.open_if_due().await
        } else {
            None
        };
        assert!(
            opened_app.is_none() || failure.cause() == CredentialCause::AppNotRunning,
            "the app was opened after an attempt that found it running"
        );
        let posting = notifier.notify_if_due(failure.cause()).await;
        record(&CredentialAttempt {
            started_at,
            duration,
            pid: std::process::id(),
            attempt,
            failure,
            opened_app,
            notified: posting,
            retry_in,
        });
        tokio::time::sleep(retry_in?.saturating_sub(acting.elapsed())).await;
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
        match &attempt.opened_app {
            None => {}
            Some(AppOpening::Opened) => write!(f, " opened_app=yes")?,
            Some(AppOpening::Failed { reason }) => write!(
                f,
                " opened_app=failed open_error=\"{}\"",
                one_line(reason.as_str())
            )?,
        }
        match &attempt.notified {
            None => {}
            Some(Notifying::Sent) => write!(f, " notified=yes")?,
            Some(Notifying::Failed { reason }) => write!(
                f,
                " notified=failed notify_error=\"{}\"",
                one_line(reason.as_str())
            )?,
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

        let obtained = obtain(
            resolve,
            Retries::until_obtained(),
            &mut never_opens(),
            &mut never_posts(),
            move |attempt| {
                log.borrow_mut()
                    .push((attempt.failure.cause(), attempt.retry_in));
            },
        )
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

    type OpenResult = std::future::Ready<anyhow::Result<()>>;

    /// An opener whose open always succeeds, counting its calls in `opens`.
    fn counting_opener(opens: &Rc<RefCell<u32>>) -> AppOpener<impl FnMut() -> OpenResult> {
        let opens = Rc::clone(opens);
        AppOpener::new(move || {
            *opens.borrow_mut() += 1;
            std::future::ready(Ok(()))
        })
    }

    fn never_opens() -> AppOpener<impl FnMut() -> OpenResult> {
        AppOpener::new(|| -> OpenResult { panic!("opened the app") })
    }

    /// A notifier whose posting would panic, for loops that never last ten minutes.
    fn never_posts() -> Notifier<impl FnMut(Notification) -> OpenResult> {
        Notifier::new(|_| -> OpenResult { panic!("posted a notification") })
    }

    /// A notifier that records every notification it posts in `posted`, with the real timing.
    fn recording_notifier(
        posted: &Rc<RefCell<Vec<Notification>>>,
    ) -> Notifier<impl FnMut(Notification) -> OpenResult> {
        let posted = Rc::clone(posted);
        Notifier::new(move |notification| {
            posted.borrow_mut().push(notification);
            std::future::ready(Ok(()))
        })
    }

    /// What each failed attempt recorded about notifying, for `failures` each taking `takes`.
    async fn notified_for(
        failures: Vec<CredentialFailure>,
        takes: Duration,
        notifier: &mut Notifier<impl FnMut(Notification) -> OpenResult>,
    ) -> Vec<Option<Notifying>> {
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&recorded);
        let resolve = failing_slowly_then(failures, takes, "secrets");
        let obtained = obtain(
            resolve,
            Retries::until_obtained(),
            &mut never_opens(),
            notifier,
            move |attempt| log.borrow_mut().push(attempt.notified.clone()),
        )
        .await;
        assert_eq!(obtained, Some("secrets"));
        let recorded = recorded.borrow().clone();
        recorded
    }

    const NETWORK: &str = "dial tcp: lookup my.1password.com: no such host";

    #[tokio::test(start_paused = true)]
    async fn nothing_is_posted_before_ten_minutes_and_one_is_after() {
        let posted = Rc::new(RefCell::new(Vec::new()));
        // Failures end at about 4, 8, 12 and 16 minutes; ten minutes after the first is 14.
        let notified = notified_for(
            vec![
                failure(NETWORK),
                failure(NETWORK),
                failure(NETWORK),
                failure(NETWORK),
            ],
            Duration::from_secs(4 * 60),
            &mut recording_notifier(&posted),
        )
        .await;

        assert_eq!(notified, vec![None, None, None, Some(Notifying::Sent)]);
        assert_eq!(
            *posted.borrow(),
            vec![Notification::for_cause(CredentialCause::Unreachable)]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn while_credentials_stay_missing_another_is_posted_each_hour() {
        let posted = Rc::new(RefCell::new(Vec::new()));
        // Failures end about every 25 minutes: at 25, 50, 75, 100, 125 and 150.
        let notified = notified_for(
            (0..6).map(|_| failure(NETWORK)).collect(),
            Duration::from_secs(25 * 60),
            &mut recording_notifier(&posted),
        )
        .await;

        assert_eq!(
            notified,
            vec![
                None,
                Some(Notifying::Sent),
                None,
                None,
                Some(Notifying::Sent),
                None
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_post_does_not_push_back_the_next_attempt() {
        let started = tokio::time::Instant::now();
        let mut slow = Notifier::with_timing(
            |_| async {
                tokio::time::sleep(Duration::from_millis(500)).await;
                Ok(())
            },
            Duration::ZERO,
            Duration::from_secs(60 * 60),
        );
        let resolve = failing_then(vec![failure(NETWORK), failure(NETWORK)], "secrets");

        let _ = obtain(
            resolve,
            Retries::until_obtained(),
            &mut never_opens(),
            &mut slow,
            |_| {},
        )
        .await;

        // The backoff's 1 s and 2 s, exactly as with nothing posted.
        assert_eq!(started.elapsed(), Duration::from_secs(3));
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_post_is_recorded_and_the_daemon_keeps_trying() {
        let mut failing = Notifier::with_timing(
            |_| std::future::ready(Err(anyhow::anyhow!("osascript exited 1"))),
            Duration::ZERO,
            Duration::from_secs(60 * 60),
        );

        let notified = notified_for(vec![failure(NETWORK)], Duration::ZERO, &mut failing).await;

        assert_eq!(
            notified,
            vec![Some(Notifying::failed(
                &anyhow::anyhow!("osascript exited 1"),
                &no_secrets()
            ))]
        );
    }

    const APP_QUIT: &str =
        "connecting to desktop app: 1Password CLI couldn't connect to the 1Password desktop app.";

    /// A resolver that fails with `failures` in order, each after `takes`, then returns
    /// `credentials`.
    fn failing_slowly_then(
        failures: Vec<CredentialFailure>,
        takes: Duration,
        credentials: &'static str,
    ) -> impl FnMut() -> std::pin::Pin<Box<dyn Future<Output = Result<&'static str, CredentialFailure>>>>
    {
        let mut failures = failures.into_iter();
        move || {
            let next = failures.next();
            Box::pin(async move {
                tokio::time::sleep(takes).await;
                next.map_or(Ok(credentials), Err)
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_app_is_opened_on_the_first_quit_attempt_and_not_again_within_a_minute() {
        let opens = Rc::new(RefCell::new(0));
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&recorded);
        let resolve = failing_then(
            vec![failure(APP_QUIT), failure(APP_QUIT), failure(APP_QUIT)],
            "secrets",
        );

        let obtained = obtain(
            resolve,
            Retries::until_obtained(),
            &mut counting_opener(&opens),
            &mut never_posts(),
            move |attempt| {
                log.borrow_mut().push(attempt.opened_app.clone());
            },
        )
        .await;

        assert_eq!(obtained, Some("secrets"));
        assert_eq!(*opens.borrow(), 1);
        assert_eq!(
            *recorded.borrow(),
            vec![Some(AppOpening::Opened), None, None]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_app_is_opened_again_once_a_minute_has_passed() {
        let opens = Rc::new(RefCell::new(0));
        // Attempts start at 0 s, about 41 s and about 83 s.
        let resolve = failing_slowly_then(
            vec![failure(APP_QUIT), failure(APP_QUIT), failure(APP_QUIT)],
            Duration::from_secs(40),
            "secrets",
        );

        let _ = obtain(
            resolve,
            Retries::until_obtained(),
            &mut counting_opener(&opens),
            &mut never_posts(),
            |_| {},
        )
        .await;

        assert_eq!(*opens.borrow(), 2);
    }

    #[rstest]
    #[case::unreachable("dial tcp: lookup my.1password.com: no such host")]
    #[case::touch_id(UNANSWERED)]
    #[case::other("\"Nope\" isn't a vault")]
    #[tokio::test(start_paused = true)]
    async fn no_other_cause_opens_the_app(#[case] message: &str) {
        let resolve = failing_then(vec![failure(message)], "secrets");

        let obtained = obtain(
            resolve,
            Retries::until_obtained(),
            &mut never_opens(),
            &mut never_posts(),
            |_| {},
        )
        .await;

        assert_eq!(obtained, Some("secrets"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_open_is_recorded_and_the_daemon_keeps_trying() {
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&recorded);
        let mut opener = AppOpener::new(|| {
            std::future::ready(Err(anyhow::anyhow!("LSOpenURLsWithRole() failed")))
        });
        let resolve = failing_then(vec![failure(APP_QUIT)], "secrets");

        let obtained = obtain(
            resolve,
            Retries::until_obtained(),
            &mut opener,
            &mut never_posts(),
            move |attempt| {
                log.borrow_mut().push(attempt.opened_app.clone());
            },
        )
        .await;

        assert_eq!(obtained, Some("secrets"));
        assert_eq!(
            *recorded.borrow(),
            vec![Some(AppOpening::failed(
                &anyhow::anyhow!("LSOpenURLsWithRole() failed"),
                &no_secrets()
            ))]
        );
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

        let obtained = obtain(
            resolve,
            Retries::until_obtained(),
            &mut never_opens(),
            &mut never_posts(),
            move |attempt| {
                log.borrow_mut().push((attempt.attempt, attempt.retry_in));
            },
        )
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

        let _ = obtain(
            resolve,
            Retries::until_obtained(),
            &mut never_opens(),
            &mut never_posts(),
            |_| {},
        )
        .await;

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

        let obtained = obtain(
            resolve,
            Retries::Never,
            &mut never_opens(),
            &mut never_posts(),
            move |attempt| {
                log.borrow_mut().push((attempt.attempt, attempt.retry_in));
            },
        )
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
                opened_app: None,
                notified: None,
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

    #[test]
    fn the_line_says_whether_the_app_was_opened() {
        let mut opened = report(Some(Duration::from_secs(1)), None);
        opened.attempt.opened_app = Some(AppOpening::Opened);
        let mut refused = report(Some(Duration::from_secs(1)), None);
        refused.attempt.opened_app = Some(AppOpening::failed(
            &anyhow::anyhow!("LSOpenURLsWithRole() failed"),
            &no_secrets(),
        ));

        assert!(
            opened
                .to_string()
                .contains(" retry_in=1s opened_app=yes error="),
            "{opened}"
        );
        assert!(
            refused.to_string().contains(
                " retry_in=1s opened_app=failed open_error=\"LSOpenURLsWithRole() failed\" error="
            ),
            "{refused}"
        );
    }

    #[test]
    fn the_line_says_whether_a_notification_was_posted() {
        let mut sent = report(Some(Duration::from_secs(1)), None);
        sent.attempt.notified = Some(Notifying::Sent);
        let mut failed = report(Some(Duration::from_secs(1)), None);
        failed.attempt.notified = Some(Notifying::failed(
            &anyhow::anyhow!("osascript exited 1"),
            &no_secrets(),
        ));

        assert!(
            sent.to_string()
                .contains(" retry_in=1s notified=yes error="),
            "{sent}"
        );
        assert!(
            failed.to_string().contains(
                " retry_in=1s notified=failed notify_error=\"osascript exited 1\" error="
            ),
            "{failed}"
        );
    }
}
