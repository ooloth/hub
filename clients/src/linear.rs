use anyhow::{Context, Result};
use chrono::Utc;
use domain::{IssueState, LinearIssue, Urgency};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct Request {
    query: String,
}

#[derive(Deserialize)]
struct Response {
    data: Data,
}

#[derive(Deserialize)]
struct Data {
    issues: IssueConnection,
}

#[derive(Deserialize)]
struct IssueConnection {
    nodes: Vec<IssueNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueNode {
    identifier: String,
    title: String,
    url: String,
    state: State,
    created_at: String,
}

#[derive(Deserialize)]
struct State {
    name: String,
}

/// Returns all incomplete issues in the workspace.
///
/// # Errors
/// Returns an error if the Linear API is unreachable or returns a non-2xx response.
pub async fn issues(token: &str) -> Result<Vec<LinearIssue>> {
    // See: https://studio.apollographql.com/public/Linear-API/variant/current/schema/reference
    let query = r#"{
        issues(filter: {
            state: { type: { nin: ["completed", "cancelled"] } }
        }) {
            nodes { identifier title url createdAt state { name } }
        }
    }"#;

    let resp = reqwest::Client::new()
        .post("https://api.linear.app/graphql")
        .header("Authorization", token)
        .json(&Request {
            query: query.to_string(),
        })
        .send()
        .await
        .context("failed to reach Linear API")?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .context("failed to read Linear response")?;

    if !status.is_success() {
        anyhow::bail!("Linear API error {status}: {body}");
    }

    let response: Response =
        serde_json::from_str(&body).context("failed to parse Linear response")?;

    Ok(response
        .data
        .issues
        .nodes
        .into_iter()
        .map(|n| LinearIssue {
            identifier: n.identifier,
            title: n.title,
            url: n.url,
            state: n.state.name,
            age: age(&n.created_at),
            urgency: Urgency::Medium,
        })
        .collect())
}

fn age(created_at: &str) -> chrono::Duration {
    let Ok(created) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return chrono::Duration::zero();
    };
    let d = Utc::now() - created.to_utc();
    if d < chrono::Duration::zero() {
        chrono::Duration::zero()
    } else {
        d
    }
}

// ── Issue state lookup (fold-back) ───────────────────────────────────────────

#[derive(Deserialize)]
struct IssueStatesResponse {
    data: IssueStatesData,
}

#[derive(Deserialize)]
struct IssueStatesData {
    issues: IssueStatesConnection,
}

#[derive(Deserialize)]
struct IssueStatesConnection {
    nodes: Vec<IssueStateNode>,
}

#[derive(Deserialize)]
struct IssueStateNode {
    identifier: String,
    state: LinearStateKind,
}

#[derive(Deserialize)]
struct LinearStateKind {
    #[serde(rename = "type")]
    kind: String,
}

/// Returns the current state of each Linear issue identified by `identifier`
/// (e.g. `"ENG-123"`).
///
/// Uses a single batched GraphQL query with an identifier filter. Identifiers
/// not found in the response are omitted from the result map — a map miss
/// means "no action" for the caller.
///
/// # Errors
/// Returns an error if the Linear API is unreachable or returns a non-2xx response.
pub async fn issue_states(
    token: &str,
    identifiers: &[String],
) -> Result<std::collections::HashMap<String, IssueState>> {
    issue_states_with_base("https://api.linear.app", token, identifiers).await
}

async fn issue_states_with_base(
    base: &str,
    token: &str,
    identifiers: &[String],
) -> Result<std::collections::HashMap<String, IssueState>> {
    if identifiers.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let id_list = identifiers
        .iter()
        .map(|id| format!(r#""{id}""#))
        .collect::<Vec<_>>()
        .join(", ");

    let query = format!(
        r#"{{
            issues(filter: {{
                identifier: {{ in: [{id_list}] }}
            }}) {{
                nodes {{ identifier state {{ type }} }}
            }}
        }}"#
    );

    let resp = reqwest::Client::new()
        .post(format!("{base}/graphql"))
        .header("Authorization", token)
        .json(&Request { query })
        .send()
        .await
        .context("failed to reach Linear API for issue states")?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .context("failed to read Linear response for issue states")?;

    if !status.is_success() {
        anyhow::bail!("Linear API error {status} for issue states: {body}");
    }

    let response: IssueStatesResponse =
        serde_json::from_str(&body).context("failed to parse Linear issue-state response")?;

    Ok(response
        .data
        .issues
        .nodes
        .into_iter()
        .map(|n| (n.identifier, parse_linear_state_type(&n.state.kind)))
        .collect())
}

fn parse_linear_state_type(kind: &str) -> IssueState {
    match kind {
        "completed" => IssueState::Completed,
        "cancelled" => IssueState::Cancelled,
        _ => IssueState::Open,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn linear_issue_states_response(nodes: &[(&str, &str)]) -> serde_json::Value {
        let nodes_json: Vec<_> = nodes
            .iter()
            .map(|(id, kind)| {
                serde_json::json!({
                    "identifier": id,
                    "state": { "type": kind }
                })
            })
            .collect();
        serde_json::json!({ "data": { "issues": { "nodes": nodes_json } } })
    }

    #[tokio::test]
    async fn issue_states_returns_completed_for_completed_state() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(linear_issue_states_response(&[("ENG-1", "completed")])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(&server.uri(), "token", &["ENG-1".to_string()])
            .await
            .unwrap();

        assert_eq!(result["ENG-1"], IssueState::Completed);
    }

    #[tokio::test]
    async fn issue_states_returns_cancelled_for_cancelled_state() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(linear_issue_states_response(&[("ENG-2", "cancelled")])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(&server.uri(), "token", &["ENG-2".to_string()])
            .await
            .unwrap();

        assert_eq!(result["ENG-2"], IssueState::Cancelled);
    }

    #[tokio::test]
    async fn issue_states_returns_open_for_active_state() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(linear_issue_states_response(&[("ENG-3", "started")])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(&server.uri(), "token", &["ENG-3".to_string()])
            .await
            .unwrap();

        assert_eq!(result["ENG-3"], IssueState::Open);
    }

    #[tokio::test]
    async fn issue_states_absent_identifier_is_map_miss() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(linear_issue_states_response(&[])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(&server.uri(), "token", &["ENG-99".to_string()])
            .await
            .unwrap();

        assert!(
            result.is_empty(),
            "expected map miss for unknown identifier"
        );
    }

    #[tokio::test]
    async fn issue_states_batches_multiple_identifiers() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(linear_issue_states_response(&[
                    ("ENG-1", "completed"),
                    ("ENG-2", "cancelled"),
                    ("ENG-3", "unstarted"),
                ])),
            )
            .mount(&server)
            .await;

        let result = issue_states_with_base(
            &server.uri(),
            "token",
            &[
                "ENG-1".to_string(),
                "ENG-2".to_string(),
                "ENG-3".to_string(),
            ],
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result["ENG-1"], IssueState::Completed);
        assert_eq!(result["ENG-2"], IssueState::Cancelled);
        assert_eq!(result["ENG-3"], IssueState::Open);
    }
}
