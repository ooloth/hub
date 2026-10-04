//! Running a pass now and then once per period, for as long as the process lives.
use std::future::Future;
use std::time::Duration;

/// Runs `pass` immediately, then once per `period`, one at a time, forever.
///
/// A pass that overruns its period, or a machine that slept through several, delays the next
/// pass rather than causing a burst of catch-up passes.
pub(crate) async fn every<F, Fut>(period: Duration, mut pass: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let mut ticks = tokio::time::interval(period);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let _ = ticks.tick().await;
        pass().await;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use tokio::time::Instant;

    use super::*;

    const FIFTEEN_MINUTES: Duration = Duration::from_secs(15 * 60);

    /// Runs `every` for `for_how_long` of virtual time and returns the minute each pass started.
    async fn pass_starts(for_how_long: Duration, pass_takes: Duration) -> Vec<u64> {
        let started = Instant::now();
        let starts = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&starts);
        let _ = tokio::time::timeout(
            for_how_long,
            every(FIFTEEN_MINUTES, move || {
                recorded.borrow_mut().push(started.elapsed().as_secs() / 60);
                tokio::time::sleep(pass_takes)
            }),
        )
        .await;
        let starts = starts.borrow().clone();
        starts
    }

    #[tokio::test(start_paused = true)]
    async fn passes_run_now_and_then_once_per_period() {
        let starts = pass_starts(Duration::from_secs(46 * 60), Duration::ZERO).await;

        assert_eq!(starts, vec![0, 15, 30, 45]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_pass_that_overruns_delays_the_next_without_a_burst() {
        let starts = pass_starts(Duration::from_secs(46 * 60), Duration::from_secs(20 * 60)).await;

        assert_eq!(starts, vec![0, 20, 40]);
    }
}
