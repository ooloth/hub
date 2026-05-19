use anyhow::{Context, Result};
use chrono::Utc;
use domain::{LinearIssue, Urgency};
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
