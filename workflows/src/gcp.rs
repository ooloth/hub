use anyhow::Result;
use chrono::Utc;
use domain::{GcpEntry, GcpEnv, GcpQuery, UntrustedText, Urgency};

/// Queries GCP Logging for each configured query and returns matching log entries.
///
/// # Errors
/// Returns an error if `gcloud` exits non-zero or its output cannot be parsed.
pub async fn run(env: &GcpEnv) -> Result<Vec<GcpEntry>> {
    let mut results = Vec::new();

    for query in &env.queries {
        let entries =
            clients::gcp::entries(&env.gcp_project, &query.query, &query.lookback).await?;

        let url = console_url(
            &env.gcp_project,
            env.gcp_region.as_deref(),
            &query.query,
            &query.lookback,
        );

        results.extend(
            entries
                .iter()
                .map(|entry| alert_entry(env, query, entry, &url)),
        );
    }

    Ok(results)
}

/// The alert one log entry raised, under the environment and query that found it.
fn alert_entry(
    env: &GcpEnv,
    query: &GcpQuery,
    entry: &clients::gcp::LogEntry,
    url: &str,
) -> GcpEntry {
    GcpEntry {
        title: query.title.clone(),
        project: env.project.clone(),
        repo: env.repo.clone(),
        env: env.env.clone(),
        message: UntrustedText::new(message_for_entry(entry, &query.message_field)),
        line: UntrustedText::new(entry.raw.clone()),
        lookback: query.lookback.clone(),
        age: age_from_entry(entry),
        urgency: Urgency::High,
        url: url.to_string(),
        gcp_project: env.gcp_project.clone(),
    }
}

fn message_for_entry(entry: &clients::gcp::LogEntry, message_field: &str) -> String {
    if let Some(payload) = &entry.json_payload {
        if let Some(msg) = payload.get(message_field).and_then(|v| v.as_str()) {
            return msg.to_string();
        }
    }
    if let Some(text) = &entry.text_payload {
        let first_line = text.lines().next().unwrap_or(text);
        return first_line.to_string();
    }
    if let Some(req) = &entry.http_request {
        let status = req.get("status").and_then(serde_json::Value::as_u64);
        let url = req.get("requestUrl").and_then(|v| v.as_str());
        if let (Some(s), Some(u)) = (status, url) {
            return format!("HTTP {s} {u}");
        }
    }
    entry.raw.clone()
}

fn console_url(
    gcp_project: &str,
    gcp_region: Option<&str>,
    filter: &str,
    lookback: &str,
) -> String {
    let duration = lookback_to_seconds(lookback);
    let encoded_filter = percent_encode(filter);
    let base = "https://console.cloud.google.com/logs/query";
    let mut url =
        format!("{base};query={encoded_filter};duration=PT{duration}S?project={gcp_project}");
    if let Some(region) = gcp_region {
        url.push_str("&region=");
        url.push_str(region);
    }
    url
}

/// Converts a lookback string like "1h", "30m", "7d" to total seconds.
fn lookback_to_seconds(lookback: &str) -> u64 {
    if let Some(s) = lookback.strip_suffix('h') {
        if let Ok(n) = s.parse::<u64>() {
            return n * 3600;
        }
    }
    if let Some(s) = lookback.strip_suffix('m') {
        if let Ok(n) = s.parse::<u64>() {
            return n * 60;
        }
    }
    if let Some(s) = lookback.strip_suffix('d') {
        if let Ok(n) = s.parse::<u64>() {
            return n * 86400;
        }
    }
    3600 // default 1h
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

fn age_from_entry(entry: &clients::gcp::LogEntry) -> chrono::Duration {
    let now = Utc::now();
    let age = now.signed_duration_since(entry.timestamp);
    if age < chrono::Duration::zero() {
        chrono::Duration::zero()
    } else {
        age
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use clients::gcp::LogEntry;
    use rstest::rstest;
    use std::collections::HashMap;

    /// A log entry with no payload of any kind.
    fn empty_entry() -> LogEntry {
        LogEntry {
            timestamp: Utc::now(),
            severity: None,
            text_payload: None,
            json_payload: None,
            http_request: None,
            resource_labels: HashMap::new(),
            raw: String::new(),
        }
    }

    /// The alert's investigation finds its clone by this repo, so it must be
    /// the configured project's, whatever the project is called.
    #[test]
    fn an_entry_carries_its_environments_repo() {
        let env = GcpEnv {
            project: "My App".to_string(),
            repo: domain::RepoSlug::new("org", "myapp"),
            env: "prod".to_string(),
            gcp_project: "my-org-prod".to_string(),
            gcp_region: None,
            queries: vec![],
        };
        let query = domain::GcpQuery {
            title: "errors".to_string(),
            query: "severity>=ERROR".to_string(),
            lookback: "1h".to_string(),
            message_field: "message".to_string(),
        };

        let entry = alert_entry(&env, &query, &empty_entry(), "https://console.invalid");

        assert_eq!(entry.repo, domain::RepoSlug::new("org", "myapp"));
    }

    #[test]
    fn message_prefers_json_payload_field() {
        let entry = LogEntry {
            json_payload: Some(serde_json::json!({"message": "something broke", "code": 500})),
            text_payload: Some("text fallback".to_string()),
            ..empty_entry()
        };
        assert_eq!(message_for_entry(&entry, "message"), "something broke");
    }

    #[test]
    fn message_uses_custom_field_name() {
        let entry = LogEntry {
            json_payload: Some(serde_json::json!({"msg": "custom field value", "code": 500})),
            ..empty_entry()
        };
        assert_eq!(message_for_entry(&entry, "msg"), "custom field value");
    }

    #[test]
    fn message_falls_back_to_text_payload_first_line() {
        let entry = LogEntry {
            text_payload: Some("line one\nline two".to_string()),
            ..empty_entry()
        };
        assert_eq!(message_for_entry(&entry, "message"), "line one");
    }

    #[test]
    fn message_falls_back_to_raw_when_no_payload() {
        let entry = LogEntry {
            raw: "raw log line content".to_string(),
            ..empty_entry()
        };
        assert_eq!(message_for_entry(&entry, "message"), "raw log line content");
    }

    #[test]
    fn message_falls_back_to_http_request_status_and_url() {
        let entry = LogEntry {
            http_request: Some(
                serde_json::json!({"status": 503, "requestUrl": "https://example.com/ingest"}),
            ),
            ..empty_entry()
        };
        assert_eq!(
            message_for_entry(&entry, "message"),
            "HTTP 503 https://example.com/ingest"
        );
    }

    #[test]
    fn message_json_payload_without_configured_key_falls_back_to_text() {
        let entry = LogEntry {
            json_payload: Some(serde_json::json!({"code": 500})),
            text_payload: Some("text fallback".to_string()),
            ..empty_entry()
        };
        assert_eq!(message_for_entry(&entry, "message"), "text fallback");
    }

    #[test]
    fn console_url_without_region() {
        let url = console_url("my-project", None, "severity>=ERROR", "1h");
        assert!(url.contains("project=my-project"));
        assert!(!url.contains("region="));
        assert!(url.contains("severity"));
        assert!(url.contains("PT3600S"));
    }

    #[test]
    fn console_url_with_region() {
        let url = console_url("my-project", Some("us-central1"), "severity>=ERROR", "1h");
        assert!(url.contains("region=us-central1"));
    }

    #[rstest]
    #[case::one_hour("1h", 3600)]
    #[case::a_day_in_hours("24h", 86400)]
    #[case::minutes("30m", 1800)]
    #[case::days("7d", 604_800)]
    #[case::unknown_defaults_to_one_hour("bad", 3600)]
    fn lookback_to_seconds_reads_each_unit(#[case] lookback: &str, #[case] seconds: u64) {
        assert_eq!(lookback_to_seconds(lookback), seconds);
    }
}
