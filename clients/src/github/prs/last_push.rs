//! When the head of a pull request's branch was pushed, from GitHub's repository activity.
//!
//! GitHub's GraphQL API no longer says when a commit was pushed (`pushedDate` is null, measured
//! 2026-10-19), and a review names the commit its reviewer was looking at rather than the head
//! when it was submitted. The activity API records each push with its time.
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::DateTime;
use domain::review_waiting::{CommitOid, LastPush};
use serde::Deserialize;

/// The longest one lookup waits, well inside the minute a whole source is given.
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Deserialize)]
struct ActivityNode {
    after: String,
    timestamp: String,
}

/// The last push to `branch` in `head_repo`, which must have left `head` at its head.
///
/// # Errors
/// When GitHub cannot be reached or refuses, when the branch has no recorded activity, or when its
/// newest activity left a commit other than `head`, as when a push lands between the two requests.
pub(super) async fn last_push_with_base(
    base: &str,
    token: &str,
    head_repo: &str,
    branch: &str,
    head: &CommitOid,
) -> Result<LastPush> {
    let newest: Vec<ActivityNode> = reqwest::Client::new()
        .get(format!("{base}/repos/{head_repo}/activity"))
        .query(&[
            ("ref", format!("refs/heads/{branch}")),
            ("per_page", "1".to_string()),
        ])
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .header("Accept", "application/vnd.github+json")
        .timeout(TIMEOUT)
        .send()
        .await
        .context("failed to reach GitHub API for repository activity")?
        .error_for_status()
        .context("GitHub API error fetching repository activity")?
        .json()
        .await
        .context("failed to parse repository activity response")?;

    let Some(push) = newest.first() else {
        bail!("{head_repo} has no recorded activity on {branch}");
    };
    if push.after != head.to_string() {
        bail!(
            "the newest activity on {head_repo} {branch} left {}, not the pull request's head {head}",
            push.after
        );
    }
    let at = DateTime::parse_from_rfc3339(&push.timestamp)
        .with_context(|| format!("unreadable activity time {:?}", push.timestamp))?
        .to_utc();
    Ok(LastPush {
        at,
        head: head.clone(),
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use wiremock::{matchers, Mock, MockServer, ResponseTemplate};

    use super::*;

    const HEAD: &str = "ffc50ad1e5b4a3c2d1f0e9d8c7b6a5f4e3d2c1b0";

    async fn server_answering(body: serde_json::Value) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/repos/ooloth/media-ops/activity"))
            .and(matchers::query_param("ref", "refs/heads/claude/issue-26"))
            .and(matchers::query_param("per_page", "1"))
            .and(matchers::header("Authorization", "Bearer token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
        server
    }

    async fn lookup(server: &MockServer) -> Result<LastPush> {
        last_push_with_base(
            &server.uri(),
            "token",
            "ooloth/media-ops",
            "claude/issue-26",
            &CommitOid::new(HEAD),
        )
        .await
    }

    #[tokio::test]
    async fn the_newest_activity_on_the_branch_is_the_last_push() {
        let server = server_answering(serde_json::json!([
            { "after": HEAD, "timestamp": "2026-05-27T05:10:36Z", "activity_type": "branch_creation" }
        ]))
        .await;

        let push = lookup(&server).await.unwrap();

        assert_eq!(
            push.at,
            "2026-05-27T05:10:36Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(push.head, CommitOid::new(HEAD));
    }

    #[tokio::test]
    async fn activity_that_left_another_commit_is_not_the_heads_push() {
        let server = server_answering(serde_json::json!([
            { "after": "0000000000000000000000000000000000000000", "timestamp": "2026-05-28T00:00:00Z", "activity_type": "branch_deletion" }
        ]))
        .await;

        let error = lookup(&server).await.unwrap_err();

        assert!(
            error.to_string().contains("not the pull request's head"),
            "{error:#}"
        );
    }

    #[tokio::test]
    async fn a_branch_with_no_activity_has_no_known_push() {
        let server = server_answering(serde_json::json!([])).await;

        let error = lookup(&server).await.unwrap_err();

        assert!(
            error.to_string().contains("no recorded activity"),
            "{error:#}"
        );
    }

    #[tokio::test]
    async fn a_refusal_is_a_failure() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let error = lookup(&server).await.unwrap_err();

        assert!(format!("{error:#}").contains("403"), "{error:#}");
    }
}
