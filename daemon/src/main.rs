//! Hub daemon — refreshes hub's signal cache with nobody present.
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use domain::profile::Profile;

use crate::freshness::RefreshOutcome;
use clap::Parser;

mod cache;
mod freshness;
mod instance_lock;
mod pass;
mod refresh;
mod schedule;

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

/// Runs one pass: asks every source, then replaces the cache if any source answered.
///
/// Never returns an error. Whatever happens is recorded in the report, so a pass that goes wrong
/// cannot end the loop that called it.
async fn run_pass(config: &config::Config, profile: Profile) -> pass::PassReport {
    let at = Utc::now();
    let started = std::time::Instant::now();
    let outcome = refresh_cache(config, profile)
        .await
        .unwrap_or_else(|error| pass::PassOutcome::Failed {
            error: format!("{error:#}"),
        });
    pass::PassReport {
        at,
        pid: std::process::id(),
        profile: profile.as_str().to_string(),
        duration: started.elapsed(),
        outcome,
    }
}

async fn refresh_cache(config: &config::Config, profile: Profile) -> Result<pass::PassOutcome> {
    let refresh = refresh::fetch(config).await?;
    let refreshed = freshness::classify(refresh);

    // Opened after the fetch returns. See refresh::fetch.
    let conn = store::status_cache::connect(profile).context("failed to open the hub database")?;
    store::status_cache::ensure_table(&conn).context("failed to prepare the status cache")?;
    cache::apply(&conn, &refreshed)?;

    Ok(match refreshed {
        RefreshOutcome::Refreshed(fresh) => pass::PassOutcome::Wrote {
            items: fresh.item_count(),
            failed: fresh.failures(),
        },
        RefreshOutcome::NothingRefreshed { failures } => {
            pass::PassOutcome::Unchanged { failed: failures }
        }
    })
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
}
