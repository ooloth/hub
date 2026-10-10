//! Hub daemon — refreshes hub's signal cache with nobody present.
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use domain::daemon_pass::{Pass, PassOutcome};
use domain::known_secrets::KnownSecrets;
use domain::pass_failure::PassFailure;
use domain::profile::Profile;

use crate::freshness::{FreshPayload, RefreshOutcome};
use clap::Parser;

mod cache;
mod freshness;
mod instance_lock;
mod pass;
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

/// A period between passes, written the way people write durations (`15m`, `20s`, `2h`).
fn parse_interval(raw: &str) -> Result<Duration, String> {
    let interval = humantime::parse_duration(raw).map_err(|error| error.to_string())?;
    if interval.is_zero() {
        return Err("must be longer than zero".to_string());
    }
    Ok(interval)
}

#[tokio::main]
async fn main() -> Result<ExitCode> {
    let cli = Cli::parse();

    // Before Config::load, which resolves credentials through `op read` and
    // blocks while 1Password is locked. A mistyped HUB_PROFILE should refuse
    // immediately rather than after that wait.
    let profile = config::profile::from_env()?;

    // Also before Config::load: a second daemon for this profile refuses before raising any
    // 1Password prompt. Bound for the life of the process, because dropping it releases it.
    let home = std::env::home_dir().context("failed to resolve home directory")?;
    let lock_path = profile.dir(&home).join("daemon.lock");
    let _instance = match instance_lock::InstanceLock::acquire(&lock_path)? {
        instance_lock::Acquired::Held(lock) => lock,
        instance_lock::Acquired::HeldElsewhere { path, pid } => {
            bail!(instance_lock::refusal(profile.as_str(), &path, pid))
        }
    };

    // Before Config::load, which can wait on 1Password prompts with no end: a startup line with
    // no pass line after it is a daemon waiting on credentials.
    println!(
        "{}",
        startup::Startup {
            at: Utc::now(),
            pid: std::process::id(),
            profile,
        }
    );

    // Once, at startup: every pass reuses these credentials, so passes raise no prompts.
    let config = config::Config::load()
        .await
        .context("failed to load hub config")?;

    if cli.once {
        let report = run_pass(&config, profile).await;
        println!("{report}");
        return Ok(if report.wrote_the_cache() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }

    let config = &config;
    schedule::every(cli.interval, move || async move {
        println!("{}", run_pass(config, profile).await);
    })
    .await;
    Ok(ExitCode::SUCCESS)
}

/// Runs one pass: asks every source, replaces the cache if anything came back, and records the
/// pass in the health record either way.
///
/// Never returns an error. Whatever happens is recorded in the report, so a pass that goes wrong
/// cannot end the loop that called it.
async fn run_pass(config: &config::Config, profile: Profile) -> pass::PassReport {
    let started_at = Utc::now();
    let started = std::time::Instant::now();
    let params = refresh::params(config);
    // Before the refresh takes the credentials, so a failure of the whole pass is redacted too.
    let secrets = workflows::status::known_secrets(&params);
    let (outcome, payload) = match refresh::fetch(params).await {
        Ok(refresh) => settle(freshness::classify(refresh), &secrets),
        Err(error) => (
            PassOutcome::Failed {
                failure: PassFailure::new(&error, &secrets),
            },
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
    let recording = cache::record(|| cache::open(profile), &pass, payload.as_ref(), &secrets);
    pass::PassReport {
        profile: profile.as_str().to_string(),
        pass,
        recording,
    }
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
