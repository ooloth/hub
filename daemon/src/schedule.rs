//! Running a pass now and then once per period, for as long as the process lives.
use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::time::Instant;

/// The longest the schedule sleeps before asking again whether a pass is due, so a pass that
/// fell due while the machine slept starts within this long of waking.
const CHECK_EVERY: Duration = Duration::from_secs(60);

/// When a pass started, on both clocks the schedule reads.
#[derive(Debug, Clone, Copy)]
struct PassStart {
    /// Tokio's monotonic clock, which stops while the machine sleeps.
    awake: Instant,
    /// The wall clock, which keeps running while the machine sleeps.
    wall: DateTime<Utc>,
}

impl PassStart {
    fn elapsed(&self, awake_now: Instant, wall_now: DateTime<Utc>) -> Elapsed {
        Elapsed {
            awake: awake_now.duration_since(self.awake),
            wall: (wall_now - self.wall).to_std().ok(),
        }
    }
}

/// How long ago a pass started, by each clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Elapsed {
    /// Time spent awake since the pass started.
    awake: Duration,
    /// Wall-clock time since the pass started, or None when the wall clock has been set back
    /// before the start.
    wall: Option<Duration>,
}

/// What the schedule does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    RunNow,
    WaitFor(Duration),
}

/// Whether the next pass is due `elapsed` after the last one started, and if not, how long to
/// sleep before asking again.
fn next_step(period: Duration, elapsed: Elapsed) -> Step {
    let awake_time_left = period.saturating_sub(elapsed.awake);
    let due_by_wall = elapsed.wall.is_some_and(|wall| wall >= period);
    if awake_time_left.is_zero() || due_by_wall {
        return Step::RunNow;
    }
    let wait = awake_time_left.min(CHECK_EVERY);
    assert!(!wait.is_zero(), "a wait before the next pass is never zero");
    assert!(
        wait <= CHECK_EVERY,
        "a wait before the next pass is at most {CHECK_EVERY:?}"
    );
    Step::WaitFor(wait)
}

/// Runs `pass` immediately, then once per `period`, one at a time, forever.
///
/// A period counts by `wall_clock` as well as by time awake, so a pass that fell due while the
/// machine slept starts within [`CHECK_EVERY`] of waking. A pass that overruns its period, or a
/// machine that slept through several, delays the next pass rather than causing a burst of
/// catch-up passes.
pub(crate) async fn every<F, Fut, C>(period: Duration, wall_clock: C, mut pass: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
    C: Fn() -> DateTime<Utc>,
{
    assert!(!period.is_zero(), "the period between passes is never zero");
    loop {
        let start = PassStart {
            awake: Instant::now(),
            wall: wall_clock(),
        };
        pass().await;
        while let Step::WaitFor(wait) =
            next_step(period, start.elapsed(Instant::now(), wall_clock()))
        {
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    const FIFTEEN_MINUTES: Duration = Duration::from_secs(15 * 60);

    proptest! {
        #[test]
        fn a_pass_is_due_once_either_clock_shows_a_period(
            period_secs in 1u64..=3 * 60 * 60,
            awake_secs in 0u64..=6 * 60 * 60,
            wall_secs in proptest::option::of(0u64..=6 * 60 * 60),
        ) {
            let period = Duration::from_secs(period_secs);
            let elapsed = Elapsed {
                awake: Duration::from_secs(awake_secs),
                wall: wall_secs.map(Duration::from_secs),
            };

            let due = elapsed.awake >= period || elapsed.wall.is_some_and(|wall| wall >= period);

            prop_assert_eq!(next_step(period, elapsed) == Step::RunNow, due);
        }

        #[test]
        fn a_wait_is_never_zero_and_never_longer_than_a_minute_or_the_awake_time_left(
            period_secs in 1u64..=3 * 60 * 60,
            awake_secs in 0u64..=6 * 60 * 60,
            wall_secs in proptest::option::of(0u64..=6 * 60 * 60),
        ) {
            let period = Duration::from_secs(period_secs);
            let elapsed = Elapsed {
                awake: Duration::from_secs(awake_secs),
                wall: wall_secs.map(Duration::from_secs),
            };

            if let Step::WaitFor(wait) = next_step(period, elapsed) {
                prop_assert!(!wait.is_zero());
                prop_assert!(wait <= CHECK_EVERY);
                let awake_time_left = period.checked_sub(elapsed.awake);
                prop_assert!(awake_time_left.is_some_and(|left| wait <= left));
            }
        }
    }

    /// A wall clock that keeps pace with tokio's paused clock, then moves by `jump` once
    /// `jump_after` of tokio time has passed, the way waking from a sleep or resetting the clock
    /// moves it.
    fn wall_clock(
        origin: Instant,
        jump_after: Duration,
        jump: chrono::Duration,
    ) -> impl Fn() -> DateTime<Utc> {
        let base = DateTime::from_timestamp(1_760_000_000, 0).unwrap();
        move || {
            let elapsed = origin.elapsed();
            let now = base + chrono::Duration::from_std(elapsed).unwrap();
            if elapsed >= jump_after {
                now + jump
            } else {
                now
            }
        }
    }

    /// A wall clock that never jumps.
    fn steady_wall_clock(origin: Instant) -> impl Fn() -> DateTime<Utc> {
        wall_clock(origin, Duration::MAX, chrono::Duration::zero())
    }

    /// Runs `every` for `for_how_long` of tokio time and returns the second each pass started.
    async fn pass_starts<C>(
        period: Duration,
        for_how_long: Duration,
        pass_takes: Duration,
        wall: impl FnOnce(Instant) -> C,
    ) -> Vec<u64>
    where
        C: Fn() -> DateTime<Utc>,
    {
        let started = Instant::now();
        let starts = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&starts);
        let _ = tokio::time::timeout(
            for_how_long,
            every(period, wall(started), move || {
                recorded.borrow_mut().push(started.elapsed().as_secs());
                tokio::time::sleep(pass_takes)
            }),
        )
        .await;
        let starts = starts.borrow().clone();
        starts
    }

    #[rstest]
    #[case::fifteen_minutes(FIFTEEN_MINUTES, vec![0, 900, 1800, 2700])]
    #[case::twenty_seconds(Duration::from_secs(20), vec![0, 20, 40, 60])]
    #[tokio::test(start_paused = true)]
    async fn passes_run_now_and_then_once_per_period(
        #[case] period: Duration,
        #[case] expected: Vec<u64>,
    ) {
        let for_how_long = period * 3 + Duration::from_secs(1);

        let starts = pass_starts(period, for_how_long, Duration::ZERO, steady_wall_clock).await;

        assert_eq!(starts, expected);
    }

    #[tokio::test(start_paused = true)]
    async fn a_pass_that_overruns_delays_the_next_without_a_burst() {
        let starts = pass_starts(
            FIFTEEN_MINUTES,
            Duration::from_secs(46 * 60),
            Duration::from_secs(20 * 60),
            steady_wall_clock,
        )
        .await;

        assert_eq!(starts, vec![0, 1200, 2400]);
    }

    #[tokio::test(start_paused = true)]
    async fn waking_from_a_long_sleep_runs_one_pass_within_a_minute_then_one_per_period() {
        // Tokio's clock stops while the machine sleeps, so a two-hour sleep shows up as the wall
        // clock jumping two hours at the moment of waking, here 5m30s after the first pass.
        let woke_at = Duration::from_secs(5 * 60 + 30);

        let starts = pass_starts(
            FIFTEEN_MINUTES,
            Duration::from_secs(25 * 60),
            Duration::ZERO,
            |origin| wall_clock(origin, woke_at, chrono::Duration::hours(2)),
        )
        .await;

        assert_eq!(starts, vec![0, 360, 360 + 900]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_wall_clock_set_back_leaves_passes_once_per_period() {
        let starts = pass_starts(
            FIFTEEN_MINUTES,
            Duration::from_secs(46 * 60),
            Duration::ZERO,
            |origin| {
                wall_clock(
                    origin,
                    Duration::from_secs(5 * 60),
                    chrono::Duration::hours(-1),
                )
            },
        )
        .await;

        assert_eq!(starts, vec![0, 900, 1800, 2700]);
    }
}
