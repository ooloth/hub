//! Hub daemon — refreshes hub's signal cache with nobody present.
use anyhow::{anyhow, Context, Result};
use clap::Parser;

mod cache;
mod freshness;
mod refresh;

/// Hub daemon binary.
#[derive(Parser)]
#[command(
    version,
    about = "Hub daemon — refreshes hub's signal cache with nobody present"
)]
struct Cli {}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = Cli::parse();

    // Before Config::load, which resolves credentials through `op read` and
    // blocks while 1Password is locked. A mistyped HUB_PROFILE should refuse
    // immediately rather than after that wait.
    let profile = config::profile::from_env()?;

    let config = config::Config::load()
        .await
        .context("failed to load hub config")?;
    let report = refresh::fetch(&config).await?;

    let outcome = freshness::classify(report);

    let conn = store::status_cache::connect(profile).context("failed to open the hub database")?;
    store::status_cache::ensure_table(&conn).context("failed to prepare the status cache")?;
    cache::apply(&conn, &outcome)?;

    match outcome {
        freshness::RefreshOutcome::Refreshed(fresh) => {
            println!(
                "hub-daemon refresh=ok profile={profile} items={} failed_sources={} schema_version={}",
                fresh.item_count(),
                fresh.failed_source_count(),
                workflows::status::SCHEMA_VERSION,
            );
            Ok(())
        }
        freshness::RefreshOutcome::NothingRefreshed { failed_sources } => Err(anyhow!(
            "every source failed, so the cache was left unchanged: {}",
            failed_sources.join(", ")
        )),
    }
}
