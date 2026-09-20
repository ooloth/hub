//! Asking every source for the current state of the queue.

use anyhow::{Context, Result};
use workflows::status::{StatusParams, StatusReport};

/// Asks every configured source once and merges the answers.
///
/// This takes no database handle on purpose. `rusqlite::Connection` is not
/// `Sync`, so holding one across this await makes the surrounding future
/// non-`Send` and unspawnable, which the interval loop and the socket server
/// will both need. Open the connection after the fetch returns.
///
/// # Errors
/// Returns an error if the refresh cannot be run at all. Individual source
/// failures are collected into `StatusReport::errors` instead.
pub(crate) async fn fetch(config: &config::Config) -> Result<StatusReport> {
    let params = StatusParams {
        github_token: config.github_token.clone(),
        github_username: config.github_username.clone(),
        pr_repos: config.github_pr_repos(),
        issue_repos: config.github_issue_repos(),
        ci_repos: config.github_ci_repos(),
        linear_token: config.linear_token.clone(),
        private_workflow_names: config.private_monitor_workflow_names(),
        loki_envs: config.loki_envs(),
        gcp_envs: config.gcp_envs(),
        extra_credentials: config.extra_credentials.clone(),
    };

    workflows::status::run(params)
        .await
        .context("failed to refresh hub signals")
}
