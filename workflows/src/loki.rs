use anyhow::Result;
use chrono::Utc;
use domain::{LokiEntry, LokiEnv, LokiQuery, UntrustedText, Urgency};
use secrecy::ExposeSecret;

/// Queries Loki for each configured query and returns matching log entries.
///
/// # Errors
/// Returns an error if the Loki HTTP request fails or the response cannot be parsed.
pub async fn run(env: &LokiEnv) -> Result<Vec<LokiEntry>> {
    let mut results = Vec::new();

    for query in &env.queries {
        let entries = clients::loki::entries(
            &env.endpoint,
            env.token.as_ref().map(|t| t.expose_secret().as_str()),
            &query.query,
            &query.lookback,
        )
        .await?;

        let url = grafana_explore_url(env.grafana_url.as_deref(), &query.query, &query.lookback);

        results.extend(
            entries
                .iter()
                .map(|entry| alert_entry(env, query, entry, &url)),
        );
    }

    Ok(results)
}

/// The alert one log line raised, under the environment and query that found it.
fn alert_entry(
    env: &LokiEnv,
    query: &LokiQuery,
    entry: &clients::loki::LogEntry,
    url: &str,
) -> LokiEntry {
    LokiEntry {
        title: query.title.clone(),
        project: env.project.clone(),
        repo: env.repo.clone(),
        env: env.env.clone(),
        message: UntrustedText::new(message_for_entry(entry, &query.message_field)),
        line: UntrustedText::new(entry.line.clone()),
        lookback: query.lookback.clone(),
        age: age_from_entry(entry),
        urgency: Urgency::High,
        url: url.to_string(),
    }
}

fn message_for_entry(entry: &clients::loki::LogEntry, message_field: &str) -> String {
    // 1. Stream label — works when message_field is a Loki selector label
    if let Some(msg) = entry.labels.get(message_field) {
        return msg.clone();
    }
    // 2. JSON log line — parses the line as JSON and extracts message_field
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&entry.line) {
        if let Some(msg) = json.get(message_field).and_then(|v| v.as_str()) {
            return msg.to_string();
        }
    }
    // 3. Structlog console format — extracts text after the "[level] " bracket
    if let Some(pos) = entry.line.find("] ") {
        let after = entry.line[pos + 2..].trim_start();
        if !after.is_empty() {
            return after.to_string();
        }
    }
    entry.line.clone()
}

fn grafana_explore_url(grafana_url: Option<&str>, query: &str, lookback: &str) -> String {
    let Some(base) = grafana_url else {
        return String::new();
    };
    let expr = query.replace('\\', r"\\").replace('"', "\\\"");
    let json = format!(
        r#"{{"datasource":"Loki","queries":[{{"refId":"A","expr":"{expr}"}}],"range":{{"from":"now-{lookback}","to":"now"}}}}"#
    );
    let encoded = percent_encode(&json);
    format!("{base}/explore?orgId=1&left={encoded}")
}

fn percent_encode(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

fn age_from_entry(entry: &clients::loki::LogEntry) -> chrono::Duration {
    let now_ns = Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or(entry.timestamp_ns);
    let age_secs = ((now_ns - entry.timestamp_ns) / 1_000_000_000).max(0);
    chrono::Duration::seconds(age_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::collections::HashMap;

    fn make_entry(labels: &[(&str, &str)], line: &str) -> clients::loki::LogEntry {
        clients::loki::LogEntry {
            timestamp_ns: 0,
            line: line.to_string(),
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<HashMap<_, _>>(),
        }
    }

    /// The alert's investigation finds its clone by this repo, so it must be
    /// the configured project's, whatever the project is called.
    #[test]
    fn an_entry_carries_its_environments_repo() {
        let env = LokiEnv {
            project: "My App".to_string(),
            repo: domain::RepoSlug::new("org", "myapp"),
            env: "prod".to_string(),
            endpoint: "https://loki.invalid".to_string(),
            token: None,
            grafana_url: None,
            queries: vec![],
        };
        let query = domain::LokiQuery {
            title: "errors".to_string(),
            query: "{app=\"myapp\"}".to_string(),
            lookback: "1h".to_string(),
            message_field: "message".to_string(),
        };

        let entry = alert_entry(
            &env,
            &query,
            &make_entry(&[], ""),
            "https://grafana.invalid",
        );

        assert_eq!(entry.repo, domain::RepoSlug::new("org", "myapp"));
    }

    #[rstest]
    #[case::configured_label_when_present(
        &[("message", "something broke"), ("app", "myapp")],
        "",
        "message",
        "something broke"
    )]
    #[case::custom_label_name(
        &[("msg", "custom field value"), ("app", "myapp")],
        "",
        "msg",
        "custom field value"
    )]
    #[case::field_from_a_json_line(
        &[("app", "myapp")],
        r#"{"message":"connection refused","level":"error","status":500}"#,
        "message",
        "connection refused"
    )]
    #[case::configured_field_from_a_json_line(
        &[("app", "myapp")],
        r#"{"event":"connection refused","level":"error"}"#,
        "event",
        "connection refused"
    )]
    #[case::text_after_the_level_bracket_in_structlog_console_format(
        &[("app", "myapp")],
        r#"pod_name="mypod" 2026-05-29 13:25:21 [warning  ] Error: connection refused   url="https://example.com""#,
        "message",
        r#"Error: connection refused   url="https://example.com""#
    )]
    #[case::raw_line_when_the_label_is_absent(
        &[("app", "myapp")],
        "raw log line content",
        "message",
        "raw log line content"
    )]
    fn the_message_is_read_from_the_entry(
        #[case] labels: &[(&str, &str)],
        #[case] line: &str,
        #[case] message_field: &str,
        #[case] expected: &str,
    ) {
        let entry = make_entry(labels, line);
        assert_eq!(message_for_entry(&entry, message_field), expected);
    }

    #[test]
    fn grafana_url_none_when_no_grafana_base() {
        assert_eq!(grafana_explore_url(None, "my query", "1h"), "");
    }

    #[test]
    fn grafana_url_encodes_query_and_lookback() {
        let url = grafana_explore_url(
            Some("https://grafana.example.com"),
            r#"{app="myapp"} | json | severity = "error""#,
            "1h",
        );
        assert!(url.starts_with("https://grafana.example.com/explore?orgId=1&left="));
        assert!(url.contains("%22datasource%22"));
        assert!(url.contains("now-1h"));
    }

    #[test]
    fn percent_encode_passes_through_unreserved_chars() {
        assert_eq!(percent_encode("abc-._~"), "abc-._~");
    }

    #[test]
    fn percent_encode_encodes_special_chars() {
        let encoded = percent_encode("{\"key\":\"val\"}");
        assert_eq!(encoded, "%7B%22key%22%3A%22val%22%7D");
    }
}
