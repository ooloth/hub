//! Hub daemon — refreshes hub's signal cache with nobody present.
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use domain::credential_attempt::CredentialAttempt;
use domain::daemon_health::Schedule;
use domain::daemon_pass::{Pass, PassOutcome};
use domain::known_secrets::KnownSecrets;
use domain::notified::Notified;
use domain::pass_failure::PassFailure;
use domain::pr_group::shown_cards;
use domain::profile::Profile;
use domain::turns_seen::TurnsSeen;

use clap::Parser;
use workflows::freshness::{self, FreshPayload, RefreshOutcome};

mod cache;
mod credentials;
mod instance_lock;
mod notification;
mod onepassword_app;
mod pass;
mod pr_notifier;
mod refresh;
mod schedule;
mod startup;

/// Hub daemon binary.
#[derive(Parser, Debug)]
#[command(
    version,
    about = "Hub daemon — refreshes hub's signal cache with nobody present"
)]
struct Cli {
    /// How long to wait between passes, such as 15m, 20s or 2h.
    #[arg(long, default_value = "15m", value_parser = parse_interval, conflicts_with = "once")]
    interval: Duration,
    /// Run one pass and exit: 0 if it wrote the cache, 1 if it did not.
    #[arg(long)]
    once: bool,
}

impl Cli {
    /// How this daemon runs its passes, recorded on every pass and attempt so a reader can tell
    /// how old a pass is too old.
    fn schedule(&self) -> Schedule {
        if self.once {
            Schedule::Once
        } else {
            Schedule::every(self.interval)
        }
    }
}

/// A period between passes, written the way people write durations (`15m`, `20s`, `2h`).
fn parse_interval(raw: &str) -> Result<Duration, String> {
    let interval = humantime::parse_duration(raw).map_err(|error| error.to_string())?;
    if interval.is_zero() {
        return Err("must be longer than zero".to_string());
    }
    Ok(interval)
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    // Before credentials are resolved, which happens through `op read` and
    // blocks while 1Password is locked. A mistyped HUB_PROFILE should refuse
    // immediately rather than after that wait.
    let profile = match config::profile::from_env() {
        Ok(profile) => profile,
        Err(error) => return failed_to_start(None, &error),
    };
    match serve(&cli, profile).await {
        Ok(code) => code,
        Err(error) => failed_to_start(Some(profile), &error),
    }
}

/// Logs a failure before the first pass, with the time it happened, and exits 1.
///
/// The error is not redacted: these failures come before any credential is resolved, and
/// `op read`'s errors name `op://` references rather than values.
fn failed_to_start(profile: Option<Profile>, error: &anyhow::Error) -> ExitCode {
    eprintln!(
        "{}",
        startup::StartupFailure {
            at: Utc::now(),
            pid: std::process::id(),
            profile,
            error: format!("{error:#}"),
        }
    );
    ExitCode::FAILURE
}

/// Everything after the profile is known: the lock, credentials, then passes until stopped, or
/// one pass with `--once`. Returns an error only before the first pass, and never for
/// credentials, which are tried again until they arrive, or with `--once`, exit 1.
async fn serve(cli: &Cli, profile: Profile) -> Result<ExitCode> {
    // Also before credentials are resolved: a second daemon for this profile refuses before raising any
    // 1Password prompt. Bound for the life of the process, because dropping it releases it.
    let home = std::env::home_dir().context("failed to resolve home directory")?;
    let lock_path = profile.dir(&home).join("daemon.lock");
    let _instance = match instance_lock::InstanceLock::acquire(&lock_path)? {
        instance_lock::Acquired::Held(lock) => lock,
        instance_lock::Acquired::HeldElsewhere { path, pid } => {
            bail!(instance_lock::refusal(profile.as_str(), &path, pid))
        }
    };

    // Before credentials are resolved, which can wait on a 1Password prompt for up to 90 seconds
    // an attempt, and up to 65 more while a Touch ID prompt is on screen: a startup line with
    // nothing after it is a daemon in its first attempt.
    println!(
        "{}",
        startup::Startup {
            at: Utc::now(),
            pid: std::process::id(),
            profile,
        }
    );

    // A broken hub.toml is not something waiting fixes, so it ends the start as before.
    let file = config::ConfigFile::read().context("failed to load hub config")?;

    // Once, at startup: every pass reuses these credentials, so passes raise no prompts. A failed
    // attempt is recorded and tried again in this process, because a restart would raise fresh
    // prompts. With --once there is one attempt.
    let resolve = || file.resolve();
    let schedule = cli.schedule();
    let record = |attempt: &CredentialAttempt| {
        let record_error =
            cache::record_credential_attempt(|| cache::open(profile), attempt, schedule);
        println!(
            "{}",
            credentials::AttemptReport {
                profile: profile.as_str().to_string(),
                attempt: attempt.clone(),
                record_error,
            }
        );
    };
    let mut opener = credentials::AppOpener::new(onepassword_app::open);
    let mut notifier = credentials::Notifier::new(notification::post, notification::remove);
    let obtained = if cli.once {
        credentials::obtain(
            resolve,
            credentials::Retries::Never,
            &mut opener,
            &mut notifier,
            record,
        )
        .await
    } else {
        credentials::obtain(
            resolve,
            credentials::Retries::until_obtained(),
            &mut opener,
            &mut notifier,
            record,
        )
        .await
    };
    let Some(config) = obtained else {
        return Ok(ExitCode::FAILURE);
    };
    // A notification about missing credentials would now be out of date.
    if let Some(removed) = notifier.clear().await {
        log_notification_removal(profile, removed);
    }

    // Before the first pass, so it can remove a card whose turn ended while no daemon was running.
    let mut pr_notifier = pr_notifier::PrNotifier::new(
        profile,
        cards_showing(profile).await,
        notification::post_turn,
        notification::remove_turn,
    );

    if cli.once {
        let (report, turns) = run_pass(&config, profile, schedule).await;
        println!("{report}");
        if let Some(turns) = turns {
            println!("{}", pr_notifier.apply(&turns).await);
        }
        return Ok(if report.wrote_the_cache() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }

    // Beside the passes, so no pass waits on terminal-notifier.
    let (send_turns, turns) = tokio::sync::watch::channel(None);
    let _notifying = tokio::spawn(pr_notifier.run(turns));

    let config = &config;
    let send_turns = &send_turns;
    schedule::every(cli.interval, Utc::now, move || async move {
        let (report, turns) = run_pass(config, profile, schedule).await;
        println!("{report}");
        if let Some(turns) = turns {
            // Sending fails only once the notifier has stopped, which only a bug can cause.
            // Exiting lets launchd start a daemon whose notifier runs.
            assert!(
                send_turns.send(Some(turns)).is_ok(),
                "the pull request notifier has stopped"
            );
        }
    })
    .await;
    Ok(ExitCode::SUCCESS)
}

/// Logs how removing the notification about missing credentials went.
fn log_notification_removal(profile: Profile, removed: Result<()>) {
    println!(
        "{}",
        credentials::NotificationRemoval {
            at: Utc::now(),
            pid: std::process::id(),
            profile: profile.as_str().to_string(),
            error: removed.err().map(|error| format!("{error:#}")),
        }
    );
}

/// The pull request cards `profile` has showing, logged. None when they cannot be listed, so a card
/// whose turn ended before this start stays until dismissed.
async fn cards_showing(profile: Profile) -> Notified {
    let found = notification::list_shown()
        .await
        .map(|listed| shown_cards(profile, &listed))
        .map_err(|error| format!("{error:#}"));
    let showing = found
        .as_ref()
        .map(|shown| Notified::from_showing(shown.prs.clone()))
        .unwrap_or_default();
    println!(
        "{}",
        pr_notifier::CardsFound {
            at: Utc::now(),
            pid: std::process::id(),
            profile,
            found,
        }
    );
    showing
}

/// Runs one pass: asks every source, replaces the cache if anything came back, and records the
/// pass in the health record either way. Also returns the pull requests it found waiting on me,
/// unless the refresh could not run at all.
///
/// Never returns an error. Whatever happens is recorded in the report, so a pass that goes wrong
/// cannot end the loop that called it.
async fn run_pass(
    config: &config::Config,
    profile: Profile,
    schedule: Schedule,
) -> (pass::PassReport, Option<TurnsSeen>) {
    let started_at = Utc::now();
    let started = std::time::Instant::now();
    let params = refresh::params(config);
    // Before the refresh takes the credentials, so a failure of the whole pass is redacted too.
    let secrets = workflows::status::known_secrets(&params);
    let (outcome, payload, turns) = match refresh::fetch(params).await {
        Ok(refresh) => {
            // From the refresh itself, not the cache it may replace: a source that answered with
            // nothing still says which turns ended.
            let turns = workflows::pr_turns::turns_seen(&refresh);
            let (outcome, payload) = settle(freshness::classify(refresh), &secrets);
            (outcome, payload, Some(turns))
        }
        Err(error) => (
            PassOutcome::Failed {
                failure: PassFailure::new(&error, &secrets),
            },
            None,
            None,
        ),
    };
    let pass = Pass {
        started_at,
        duration: started.elapsed(),
        pid: std::process::id(),
        outcome,
    };
    // Opened after the fetch returns. See refresh::fetch.
    let recording = cache::record(
        || cache::open(profile),
        &pass,
        schedule,
        payload.as_ref(),
        &secrets,
    );
    let report = pass::PassReport {
        profile: profile.as_str().to_string(),
        pass,
        recording,
    };
    (report, turns)
}

/// What a refresh means for the pass, and the payload to write when it replaces the cache.
fn settle(
    refreshed: RefreshOutcome,
    secrets: &KnownSecrets,
) -> (PassOutcome, Option<FreshPayload>) {
    match refreshed {
        RefreshOutcome::Refreshed(fresh) => match fresh.payload() {
            Ok(payload) => (
                PassOutcome::Wrote {
                    items: fresh.item_count(),
                    sources: fresh.sources(),
                },
                Some(payload),
            ),
            Err(error) => (
                PassOutcome::Failed {
                    failure: PassFailure::new(&error, secrets),
                },
                None,
            ),
        },
        RefreshOutcome::NothingRefreshed { sources } => (PassOutcome::Kept { sources }, None),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("hub-daemon").chain(args.iter().copied()))
    }

    #[rstest]
    #[case::minutes("15m", 15 * 60)]
    #[case::seconds("20s", 20)]
    #[case::hours("2h", 2 * 60 * 60)]
    fn an_interval_is_read_the_way_people_write_durations(#[case] raw: &str, #[case] secs: u64) {
        let cli = parse(&["--interval", raw]).unwrap();

        assert_eq!(cli.interval, Duration::from_secs(secs));
    }

    #[test]
    fn the_interval_defaults_to_fifteen_minutes() {
        assert_eq!(parse(&[]).unwrap().interval, Duration::from_secs(15 * 60));
    }

    #[rstest]
    #[case::zero("0s")]
    #[case::not_a_duration("soon")]
    fn a_bad_interval_is_refused_naming_the_flag(#[case] raw: &str) {
        let error = parse(&["--interval", raw]).unwrap_err().to_string();

        assert!(error.contains("--interval"), "{error}");
    }

    #[test]
    fn once_and_an_interval_together_are_refused() {
        assert!(parse(&["--once", "--interval", "20s"]).is_err());
    }

    #[test]
    fn once_alone_is_accepted() {
        assert!(parse(&["--once"]).unwrap().once);
    }

    fn refresh(items: usize, answered: &[&str], failed: &[&str]) -> workflows::status::Refresh {
        let no_secrets = KnownSecrets::new(Vec::new());
        let failures: Vec<domain::source_failure::SourceFailure> = failed
            .iter()
            .map(|name| {
                domain::source_failure::SourceFailure::new(
                    *name,
                    &anyhow::anyhow!("down"),
                    &no_secrets,
                )
            })
            .collect();
        workflows::status::Refresh {
            report: workflows::status::StatusReport {
                items: (0..items)
                    .map(|_| {
                        workflows::status::StatusItem::Ci(domain::CiFailure {
                            repo: domain::RepoSlug::new("owner", "repo"),
                            workflow_name: "CI".to_string(),
                            job_name: None,
                            step_name: None,
                            error: None,
                            age: chrono::Duration::zero(),
                            urgency: domain::Urgency::High,
                            url: "https://github.com/owner/repo/actions/runs/1".to_string(),
                        })
                    })
                    .collect(),
                errors: failed.iter().map(|name| (*name).to_string()).collect(),
            },
            sources: domain::source_outcomes::SourceOutcomes::new(
                answered.iter().map(|name| (*name).to_string()).collect(),
                failures,
            ),
        }
    }

    #[test]
    fn a_refresh_worth_caching_becomes_a_pass_that_wrote_it_with_its_payload() {
        let refreshed = freshness::classify(refresh(2, &["a"], &["b"]));

        let (outcome, payload) = settle(refreshed, &KnownSecrets::new(Vec::new()));

        assert!(
            matches!(&outcome, PassOutcome::Wrote { items: 2, sources }
                if sources.answered() == ["a"] && sources.failed().len() == 1),
            "{outcome:?}"
        );
        assert!(payload.is_some());
    }

    #[test]
    fn a_refresh_not_worth_caching_becomes_a_pass_that_kept_the_payload_with_none() {
        let refreshed = freshness::classify(refresh(0, &[], &["a"]));

        let (outcome, payload) = settle(refreshed, &KnownSecrets::new(Vec::new()));

        assert!(
            matches!(&outcome, PassOutcome::Kept { sources } if sources.failed().len() == 1),
            "{outcome:?}"
        );
        assert!(payload.is_none());
    }
}
