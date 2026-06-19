use anyhow::{Context, Result};
use domain::{IssueState, RepoSlug};

/// Returns the current state of each issue identified by `(repo, number)`.
///
/// Uses a single batched GraphQL query with dynamic aliases — the same
/// approach as `pr_states`. Issues not found in the response are omitted from
/// the result map — a map miss means "no action" for the caller.
///
/// # Errors
/// Returns an error only for network failures, HTTP errors, or a malformed
/// response body. An issue GitHub can't find produces a null node; those are
/// silently skipped rather than failing the whole batch.
pub async fn issue_states(
    token: &str,
    issues: &[(RepoSlug, u64)],
) -> Result<std::collections::HashMap<(RepoSlug, u64), IssueState>> {
    issue_states_with_base("https://api.github.com", token, issues).await
}

/// Internal entry point that accepts a custom base URL for testing with wiremock.
async fn issue_states_with_base(
    base: &str,
    token: &str,
    issues: &[(RepoSlug, u64)],
) -> Result<std::collections::HashMap<(RepoSlug, u64), IssueState>> {
    if issues.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    // Group issues by repo to query each repository node once.
    let mut repo_map: Vec<(RepoSlug, Vec<u64>)> = Vec::new();
    for (repo, number) in issues {
        match repo_map.iter_mut().find(|(r, _)| r == repo) {
            Some((_, numbers)) => numbers.push(*number),
            None => repo_map.push((repo.clone(), vec![*number])),
        }
    }

    // Build a batched GraphQL query using dynamic aliases (r{i}/i{j}).
    // Each issue node requests both `state` and `stateReason` so we can
    // distinguish completed from not-planned closes.
    let mut repo_parts = Vec::new();
    for (i, (repo, numbers)) in repo_map.iter().enumerate() {
        let slug = repo.to_string();
        let (owner, name) = slug
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("invalid repo slug: {repo}"))?;

        let issue_fields: Vec<String> = numbers
            .iter()
            .enumerate()
            .map(|(j, number)| format!("i{j}: issue(number: {number}) {{ state stateReason }}"))
            .collect();

        repo_parts.push(format!(
            r#"r{i}: repository(owner: "{owner}", name: "{name}") {{ {} }}"#,
            issue_fields.join(" ")
        ));
    }

    let query = format!("{{ {} }}", repo_parts.join(" "));

    let body: serde_json::Value = reqwest::Client::new()
        .post(format!("{base}/graphql"))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await
        .context("failed to reach GitHub GraphQL API for issue states")?
        .error_for_status()
        .context("GitHub GraphQL API returned an error for issue states")?
        .json()
        .await
        .context("failed to parse GitHub GraphQL response for issue states")?;

    if let Some(errors) = body.get("errors") {
        anyhow::bail!("GitHub GraphQL returned errors for issue states: {errors}");
    }

    let data = body
        .get("data")
        .context("GitHub GraphQL response missing 'data' field")?;

    let mut result = std::collections::HashMap::new();
    for (i, (repo, numbers)) in repo_map.iter().enumerate() {
        let repo_node = match data.get(format!("r{i}")) {
            Some(v) if !v.is_null() => v,
            _ => continue,
        };
        for (j, number) in numbers.iter().enumerate() {
            let issue_node = match repo_node.get(format!("i{j}")) {
                Some(v) if !v.is_null() => v,
                _ => continue,
            };
            let state = issue_node.get("state").and_then(|s| s.as_str());
            let state_reason = issue_node.get("stateReason").and_then(|s| s.as_str());
            if let Some(s) = parse_issue_state(state, state_reason) {
                result.insert((repo.clone(), *number), s);
            }
        }
    }

    Ok(result)
}

/// Maps a GitHub issue's `state` + `stateReason` pair to the normalised
/// `IssueState`. Returns `None` when `state` is absent or unrecognised.
///
/// Mapping:
/// - `OPEN` → `Open` (stateReason ignored — only set on closed issues)
/// - `CLOSED` + `NOT_PLANNED` → `Cancelled`
/// - `CLOSED` + anything else (COMPLETED, REOPENED, null) → `Completed`
fn parse_issue_state(state: Option<&str>, state_reason: Option<&str>) -> Option<IssueState> {
    match state? {
        "OPEN" => Some(IssueState::Open),
        "CLOSED" => Some(match state_reason {
            Some("NOT_PLANNED") => IssueState::Cancelled,
            _ => IssueState::Completed,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers, Mock, MockServer, ResponseTemplate};

    // ── parse_issue_state ─────────────────────────────────────────────────────

    #[rstest::rstest]
    #[case(Some("OPEN"), None, Some(IssueState::Open))]
    #[case(Some("OPEN"), Some("COMPLETED"), Some(IssueState::Open))]
    #[case(Some("CLOSED"), Some("COMPLETED"), Some(IssueState::Completed))]
    #[case(Some("CLOSED"), Some("NOT_PLANNED"), Some(IssueState::Cancelled))]
    #[case(Some("CLOSED"), Some("REOPENED"), Some(IssueState::Completed))]
    #[case(Some("CLOSED"), None, Some(IssueState::Completed))]
    #[case(Some("UNKNOWN"), None, None)]
    #[case(None, None, None)]
    fn parse_issue_state_cases(
        #[case] state: Option<&str>,
        #[case] state_reason: Option<&str>,
        #[case] expected: Option<IssueState>,
    ) {
        assert_eq!(parse_issue_state(state, state_reason), expected);
    }

    // ── issue_states_with_base ────────────────────────────────────────────────

    fn graphql_issue_response(
        alias_issues: &[(&str, &[(&str, &str, Option<&str>)])],
    ) -> serde_json::Value {
        let mut data = serde_json::Map::new();
        for (repo_alias, issues) in alias_issues {
            let mut repo_obj = serde_json::Map::new();
            for (issue_alias, state, state_reason) in *issues {
                let mut node = serde_json::Map::new();
                node.insert(
                    "state".to_string(),
                    serde_json::Value::String(state.to_string()),
                );
                node.insert(
                    "stateReason".to_string(),
                    match state_reason {
                        Some(r) => serde_json::Value::String(r.to_string()),
                        None => serde_json::Value::Null,
                    },
                );
                repo_obj.insert(issue_alias.to_string(), serde_json::Value::Object(node));
            }
            data.insert(repo_alias.to_string(), serde_json::Value::Object(repo_obj));
        }
        serde_json::json!({ "data": data })
    }

    #[tokio::test]
    async fn issue_states_returns_completed_for_closed_completed_issue() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_issue_response(&[(
                    "r0",
                    &[("i0", "CLOSED", Some("COMPLETED"))],
                )])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 42)],
        )
        .await
        .unwrap();

        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 42)],
            IssueState::Completed
        );
    }

    #[tokio::test]
    async fn issue_states_returns_cancelled_for_not_planned_issue() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_issue_response(&[(
                    "r0",
                    &[("i0", "CLOSED", Some("NOT_PLANNED"))],
                )])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 7)],
        )
        .await
        .unwrap();

        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 7)],
            IssueState::Cancelled
        );
    }

    #[tokio::test]
    async fn issue_states_returns_open_for_open_issue() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(graphql_issue_response(&[("r0", &[("i0", "OPEN", None)])])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 1)],
        )
        .await
        .unwrap();

        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 1)],
            IssueState::Open
        );
    }

    #[tokio::test]
    async fn issue_states_completed_fallback_for_null_state_reason() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(graphql_issue_response(&[("r0", &[("i0", "CLOSED", None)])])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 3)],
        )
        .await
        .unwrap();

        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 3)],
            IssueState::Completed
        );
    }

    #[tokio::test]
    async fn issue_states_null_issue_node_is_skipped() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "data": { "r0": { "i0": serde_json::Value::Null } }
        });
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 999)],
        )
        .await
        .unwrap();

        assert!(result.is_empty(), "expected map miss for not-found issue");
    }

    #[tokio::test]
    async fn issue_states_batches_multiple_issues_across_repos() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_issue_response(&[
                    (
                        "r0",
                        &[("i0", "CLOSED", Some("COMPLETED")), ("i1", "OPEN", None)],
                    ),
                    ("r1", &[("i0", "CLOSED", Some("NOT_PLANNED"))]),
                ])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[
                (RepoSlug::new("owner", "repo-a"), 10),
                (RepoSlug::new("owner", "repo-a"), 11),
                (RepoSlug::new("owner", "repo-b"), 5),
            ],
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(
            result[&(RepoSlug::new("owner", "repo-a"), 10)],
            IssueState::Completed
        );
        assert_eq!(
            result[&(RepoSlug::new("owner", "repo-a"), 11)],
            IssueState::Open
        );
        assert_eq!(
            result[&(RepoSlug::new("owner", "repo-b"), 5)],
            IssueState::Cancelled
        );
    }
}
