use anyhow::{Context, Result};
use domain::{PrState, RepoSlug};

/// Returns the current state of each pull request identified by `(repo, number)`.
///
/// Uses a single batched GraphQL query. PRs not found in the response are
/// omitted from the result map — a map miss means "no action" for the caller.
///
/// # Errors
/// Returns an error only for network failures, HTTP errors, or a malformed
/// response body. A PR GitHub can't find produces a null node; those are
/// silently skipped rather than failing the whole batch.
pub async fn pr_states(
    token: &str,
    prs: &[(RepoSlug, u64)],
) -> Result<std::collections::HashMap<(RepoSlug, u64), PrState>> {
    pr_states_with_base("https://api.github.com", token, prs).await
}

/// Internal entry point that accepts a custom base URL for testing with wiremock.
async fn pr_states_with_base(
    base: &str,
    token: &str,
    prs: &[(RepoSlug, u64)],
) -> Result<std::collections::HashMap<(RepoSlug, u64), PrState>> {
    if prs.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    // Group PRs by repo so we query each repository node once.
    // We use a Vec (not a HashMap) to preserve insertion order — the order
    // determines the alias indices (r0, r1, …) and we need to reconstruct
    // that same mapping when we read the response.
    let mut repo_map: Vec<(RepoSlug, Vec<u64>)> = Vec::new();
    for (repo, number) in prs {
        match repo_map.iter_mut().find(|(r, _)| r == repo) {
            Some((_, numbers)) => numbers.push(*number),
            None => repo_map.push((repo.clone(), vec![*number])),
        }
    }

    // Build a batched GraphQL query using dynamic aliases.
    //
    // Why aliases, not search?
    // All existing PR queries use `search(query: "is:open is:pr …")`, which
    // only returns open PRs. Merged and closed PRs vanish from those results,
    // so detecting a merge via absence is unreliable (a transient API error
    // also looks like "gone"). Instead we query each PR directly by identity:
    //   repository(owner: "ooloth", name: "hub") {
    //     pullRequest(number: 42) { state }
    //   }
    //
    // Why dynamic aliases?
    // GraphQL requires every field returned to have a unique name within its
    // parent object. Querying multiple repositories — or multiple PRs within
    // a repo — in a single document would produce duplicate `repository` /
    // `pullRequest` keys without aliases. Aliases rename each field for the
    // duration of that query:
    //
    //   r0: repository(owner: "ooloth", name: "hub") {
    //     p0: pullRequest(number: 42) { state }
    //     p1: pullRequest(number: 99) { state }
    //   }
    //   r1: repository(owner: "other", name: "repo") {
    //     p0: pullRequest(number: 7) { state }
    //   }
    //
    // The `r{i}` / `p{j}` scheme encodes the position of each (repo, number)
    // pair, so we can map the response back to the original inputs.
    let mut repo_parts = Vec::new();
    for (i, (repo, numbers)) in repo_map.iter().enumerate() {
        let slug = repo.to_string();
        let (owner, name) = slug
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("invalid repo slug: {repo}"))?;

        let pr_fields: Vec<String> = numbers
            .iter()
            .enumerate()
            .map(|(j, number)| format!("p{j}: pullRequest(number: {number}) {{ state }}"))
            .collect();

        repo_parts.push(format!(
            r#"r{i}: repository(owner: "{owner}", name: "{name}") {{ {} }}"#,
            pr_fields.join(" ")
        ));
    }

    let query = format!("{{ {} }}", repo_parts.join(" "));

    // serde_json::Value is used here (not a fixed struct) because the field
    // names in the response are the dynamic aliases we generated above — they
    // aren't known at compile time, so no static schema can describe them.
    let body: serde_json::Value = reqwest::Client::new()
        .post(format!("{base}/graphql"))
        .bearer_auth(token)
        .header("User-Agent", "hub-cli")
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await
        .context("failed to reach GitHub GraphQL API for PR states")?
        .error_for_status()
        .context("GitHub GraphQL API returned an error for PR states")?
        .json()
        .await
        .context("failed to parse GitHub GraphQL response for PR states")?;

    // GraphQL can return HTTP 200 with an "errors" array for partial or total
    // failures. Treat any top-level errors as a hard failure — the caller
    // (fold-back) will skip this refresh cycle rather than misclassify tasks.
    if let Some(errors) = body.get("errors") {
        anyhow::bail!("GitHub GraphQL returned errors for PR states: {errors}");
    }

    let data = body
        .get("data")
        .context("GitHub GraphQL response missing 'data' field")?;

    // Walk the same r{i}/p{j} tree we built when constructing the query.
    // A null repo node means the repo was not found; a null PR node means the
    // PR was not found. Both are skipped — a map miss is the caller's cue to
    // take no action on that task.
    let mut result = std::collections::HashMap::new();
    for (i, (repo, numbers)) in repo_map.iter().enumerate() {
        let repo_node = match data.get(format!("r{i}")) {
            Some(v) if !v.is_null() => v,
            _ => continue,
        };
        for (j, number) in numbers.iter().enumerate() {
            let pr_node = match repo_node.get(format!("p{j}")) {
                Some(v) if !v.is_null() => v,
                _ => continue,
            };
            if let Some(state) = parse_pr_state(pr_node.get("state").and_then(|s| s.as_str())) {
                result.insert((repo.clone(), *number), state);
            }
        }
    }

    Ok(result)
}

fn parse_pr_state(s: Option<&str>) -> Option<PrState> {
    match s? {
        "OPEN" => Some(PrState::Open),
        "MERGED" => Some(PrState::Merged),
        "CLOSED" => Some(PrState::Closed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers, Mock, MockServer, ResponseTemplate};

    // ── parse_pr_state ────────────────────────────────────────────────────────

    #[rstest::rstest]
    #[case(Some("OPEN"), Some(PrState::Open))]
    #[case(Some("MERGED"), Some(PrState::Merged))]
    #[case(Some("CLOSED"), Some(PrState::Closed))]
    #[case(Some("UNKNOWN"), None)]
    #[case(Some(""), None)]
    #[case(None, None)]
    fn parse_pr_state_cases(#[case] input: Option<&str>, #[case] expected: Option<PrState>) {
        assert_eq!(parse_pr_state(input), expected);
    }

    // ── pr_states_with_base ───────────────────────────────────────────────────

    fn graphql_pr_response(alias_states: &[(&str, &str, &[(&str, &str)])]) -> serde_json::Value {
        // Builds the response JSON that GitHub would return for a batched alias query.
        // alias_states: [(repo_alias, _, [(pr_alias, state), ...])]
        let mut data = serde_json::Map::new();
        for (repo_alias, _, prs) in alias_states {
            let mut repo_obj = serde_json::Map::new();
            for (pr_alias, state) in *prs {
                repo_obj.insert(pr_alias.to_string(), serde_json::json!({ "state": state }));
            }
            data.insert(repo_alias.to_string(), serde_json::Value::Object(repo_obj));
        }
        serde_json::json!({ "data": data })
    }

    #[tokio::test]
    async fn pr_states_returns_merged_for_merged_pr() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_pr_response(&[(
                    "r0",
                    "owner/repo",
                    &[("p0", "MERGED")],
                )])),
            )
            .mount(&server)
            .await;

        let result = pr_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 42)],
        )
        .await
        .unwrap();

        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 42)],
            PrState::Merged
        );
    }

    #[tokio::test]
    async fn pr_states_returns_closed_for_closed_pr() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_pr_response(&[(
                    "r0",
                    "owner/repo",
                    &[("p0", "CLOSED")],
                )])),
            )
            .mount(&server)
            .await;

        let result = pr_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 7)],
        )
        .await
        .unwrap();

        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 7)],
            PrState::Closed
        );
    }

    #[tokio::test]
    async fn pr_states_returns_open_for_open_pr() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_pr_response(&[(
                    "r0",
                    "owner/repo",
                    &[("p0", "OPEN")],
                )])),
            )
            .mount(&server)
            .await;

        let result = pr_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 1)],
        )
        .await
        .unwrap();

        assert_eq!(result[&(RepoSlug::new("owner", "repo"), 1)], PrState::Open);
    }

    #[tokio::test]
    async fn pr_states_batches_multiple_prs_in_same_repo() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_pr_response(&[(
                    "r0",
                    "owner/repo",
                    &[("p0", "MERGED"), ("p1", "CLOSED")],
                )])),
            )
            .mount(&server)
            .await;

        let result = pr_states_with_base(
            &server.uri(),
            "token",
            &[
                (RepoSlug::new("owner", "repo"), 10),
                (RepoSlug::new("owner", "repo"), 11),
            ],
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 10)],
            PrState::Merged
        );
        assert_eq!(
            result[&(RepoSlug::new("owner", "repo"), 11)],
            PrState::Closed
        );
    }

    #[tokio::test]
    async fn pr_states_null_pr_node_is_skipped() {
        let server = MockServer::start().await;
        // GitHub returns null for a pullRequest node when the PR doesn't exist.
        let body = serde_json::json!({
            "data": { "r0": { "p0": serde_json::Value::Null } }
        });
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let result = pr_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 999)],
        )
        .await
        .unwrap();

        assert!(result.is_empty(), "expected map miss for not-found PR");
    }

    #[tokio::test]
    async fn pr_states_graphql_errors_field_returns_err() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "errors": [{ "message": "something went wrong" }]
        });
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let result = pr_states_with_base(
            &server.uri(),
            "token",
            &[(RepoSlug::new("owner", "repo"), 42)],
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn pr_states_empty_input_makes_no_http_call() {
        // No mock registered — any HTTP call would panic the test.
        let server = MockServer::start().await;
        let result = pr_states_with_base(&server.uri(), "token", &[])
            .await
            .unwrap();
        assert!(result.is_empty());
    }
}
