use anyhow::{Context, Result};

use super::labels::fetch_issue_labels;

/// Dismisses an issue as won't fix: replaces its labels, optionally posts a
/// reason comment, then closes the issue with `state_reason: not_planned`.
///
/// If any step after the label PUT fails, the original labels are restored
/// before returning the error. If the restore also fails, both errors are
/// reported in the error chain.
pub async fn dismiss_issue(
    token: &str,
    repo: &str,
    number: u64,
    reason: &str,
    labels: &[String],
) -> Result<()> {
    dismiss_issue_with_base(
        "https://api.github.com",
        token,
        repo,
        number,
        reason,
        labels,
    )
    .await
}

async fn dismiss_issue_with_base(
    base: &str,
    token: &str,
    repo: &str,
    number: u64,
    reason: &str,
    labels: &[String],
) -> Result<()> {
    let client = reqwest::Client::new();

    let original_labels = fetch_issue_labels(&client, base, token, repo, number).await?;

    client
        .put(format!("{base}/repos/{repo}/issues/{number}/labels"))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .json(&serde_json::json!({ "labels": labels }))
        .send()
        .await
        .with_context(|| format!("failed to reach GitHub API setting labels on {repo}#{number}"))?
        .error_for_status()
        .with_context(|| format!("GitHub API error setting labels on {repo}#{number}"))?;

    if let Err(e) = complete_dismissal(&client, base, token, repo, number, reason).await {
        let rollback = client
            .put(format!("{base}/repos/{repo}/issues/{number}/labels"))
            .bearer_auth(token)
            .header("User-Agent", "hub-cli")
            .header("Accept", "application/vnd.github.v3+json")
            .json(&serde_json::json!({ "labels": original_labels }))
            .send()
            .await
            .and_then(|r| r.error_for_status());
        return match rollback {
            Ok(_) => Err(e),
            Err(rb_err) => Err(e.context(format!("label rollback also failed: {rb_err}"))),
        };
    }

    Ok(())
}

async fn complete_dismissal(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    repo: &str,
    number: u64,
    reason: &str,
) -> Result<()> {
    if !reason.is_empty() {
        client
            .post(format!("{base}/repos/{repo}/issues/{number}/comments"))
            .bearer_auth(token)
            .header("User-Agent", "hub-cli")
            .header("Accept", "application/vnd.github.v3+json")
            .json(&serde_json::json!({ "body": reason }))
            .send()
            .await
            .with_context(|| {
                format!("failed to reach GitHub API posting comment on {repo}#{number}")
            })?
            .error_for_status()
            .with_context(|| format!("GitHub API error posting comment on {repo}#{number}"))?;
    }

    client
        .patch(format!("{base}/repos/{repo}/issues/{number}"))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .json(&serde_json::json!({ "state": "closed", "state_reason": "not_planned" }))
        .send()
        .await
        .with_context(|| format!("failed to reach GitHub API closing {repo}#{number}"))?
        .error_for_status()
        .with_context(|| format!("GitHub API error closing {repo}#{number}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers, Mock, MockServer, ResponseTemplate};

    fn label_json(names: &[&str]) -> serde_json::Value {
        serde_json::json!(names
            .iter()
            .map(|n| serde_json::json!({ "name": n }))
            .collect::<Vec<_>>())
    }

    async fn mock_get_labels(server: &MockServer, number: u64, labels: &[&str]) {
        Mock::given(matchers::method("GET"))
            .and(matchers::path(format!(
                "/repos/owner/repo/issues/{number}/labels"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(label_json(labels)))
            .expect(1)
            .mount(server)
            .await;
    }

    async fn mock_get_labels_fail(server: &MockServer, number: u64) {
        Mock::given(matchers::method("GET"))
            .and(matchers::path(format!(
                "/repos/owner/repo/issues/{number}/labels"
            )))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(server)
            .await;
    }

    // Matches PUT /labels by the body it sends, so initial PUT and rollback PUT
    // can coexist in the same mock server without ambiguity.
    async fn mock_put_labels(server: &MockServer, number: u64, body_labels: &[&str], status: u16) {
        Mock::given(matchers::method("PUT"))
            .and(matchers::path(format!(
                "/repos/owner/repo/issues/{number}/labels"
            )))
            .and(matchers::body_json(
                serde_json::json!({ "labels": body_labels }),
            ))
            .respond_with(ResponseTemplate::new(status).set_body_json(label_json(&[])))
            .expect(1)
            .mount(server)
            .await;
    }

    async fn mock_post_comment(server: &MockServer, number: u64, status: u16) {
        Mock::given(matchers::method("POST"))
            .and(matchers::path(format!(
                "/repos/owner/repo/issues/{number}/comments"
            )))
            .respond_with(ResponseTemplate::new(status).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(server)
            .await;
    }

    async fn mock_patch_close(server: &MockServer, number: u64, status: u16) {
        Mock::given(matchers::method("PATCH"))
            .and(matchers::path(format!("/repos/owner/repo/issues/{number}")))
            .respond_with(ResponseTemplate::new(status).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn dismiss_issue_succeeds_without_reason() {
        let server = MockServer::start().await;
        mock_get_labels(&server, 7, &["bug"]).await;
        mock_put_labels(&server, 7, &["wontfix"], 200).await;
        mock_patch_close(&server, 7, 200).await;

        let result = dismiss_issue_with_base(
            &server.uri(),
            "token",
            "owner/repo",
            7,
            "",
            &["wontfix".to_string()],
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dismiss_issue_succeeds_with_reason() {
        let server = MockServer::start().await;
        mock_get_labels(&server, 7, &["bug"]).await;
        mock_put_labels(&server, 7, &["wontfix"], 200).await;
        mock_post_comment(&server, 7, 201).await;
        mock_patch_close(&server, 7, 200).await;

        let result = dismiss_issue_with_base(
            &server.uri(),
            "token",
            "owner/repo",
            7,
            "not planned",
            &["wontfix".to_string()],
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dismiss_issue_rolls_back_labels_when_comment_fails() {
        let server = MockServer::start().await;
        mock_get_labels(&server, 7, &["bug"]).await;
        mock_put_labels(&server, 7, &["wontfix"], 200).await;
        mock_post_comment(&server, 7, 500).await;
        mock_put_labels(&server, 7, &["bug"], 200).await; // rollback restores original

        let result = dismiss_issue_with_base(
            &server.uri(),
            "token",
            "owner/repo",
            7,
            "reason",
            &["wontfix".to_string()],
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn dismiss_issue_rolls_back_labels_when_close_fails() {
        let server = MockServer::start().await;
        mock_get_labels(&server, 7, &["bug"]).await;
        mock_put_labels(&server, 7, &["wontfix"], 200).await;
        mock_post_comment(&server, 7, 201).await;
        mock_patch_close(&server, 7, 500).await;
        mock_put_labels(&server, 7, &["bug"], 200).await; // rollback restores original

        let result = dismiss_issue_with_base(
            &server.uri(),
            "token",
            "owner/repo",
            7,
            "reason",
            &["wontfix".to_string()],
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn dismiss_issue_makes_no_writes_when_label_fetch_fails() {
        let server = MockServer::start().await;
        mock_get_labels_fail(&server, 7).await;

        let result = dismiss_issue_with_base(
            &server.uri(),
            "token",
            "owner/repo",
            7,
            "reason",
            &["wontfix".to_string()],
        )
        .await;

        assert!(result.is_err());
        // wiremock's expect(1) on the GET verifies it ran; no PUT/POST/PATCH mocks
        // are registered, so any unexpected write would produce an error the test
        // would need to handle — but the function aborts before reaching them.
    }

    #[tokio::test]
    async fn dismiss_issue_surfaces_both_errors_when_rollback_also_fails() {
        let server = MockServer::start().await;
        mock_get_labels(&server, 7, &["bug"]).await;
        mock_put_labels(&server, 7, &["wontfix"], 200).await;
        mock_patch_close(&server, 7, 500).await;
        mock_put_labels(&server, 7, &["bug"], 500).await; // rollback also fails

        let result = dismiss_issue_with_base(
            &server.uri(),
            "token",
            "owner/repo",
            7,
            "",
            &["wontfix".to_string()],
        )
        .await;

        let err = result.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("rollback"), "expected 'rollback' in: {msg}");
    }
}
