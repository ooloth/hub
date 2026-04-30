use anyhow::{Context, Result};
use domain::LinearIssue;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct Request<'a> {
    query: &'a str,
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
struct IssueNode {
    identifier: String,
    title: String,
    url: String,
    state: State,
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
            nodes { identifier title url state { name } }
        }
    }"#;

    let resp = reqwest::Client::new()
        .post("https://api.linear.app/graphql")
        .header("Authorization", token)
        .json(&Request { query })
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
        })
        .collect())
}
