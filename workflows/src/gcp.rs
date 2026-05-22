use anyhow::Result;
use chrono::Utc;
use domain::{GcpEntry, GcpEnv, Urgency};

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

        for entry in &entries {
            let message = message_for_entry(entry);

            results.push(GcpEntry {
                title: query.title.clone(),
                project: env.project.clone(),
                env: env.env.clone(),
                message,
                line: entry.raw.clone(),
                lookback: query.lookback.clone(),
                age: age_from_entry(entry),
                urgency: Urgency::High,
                url: url.clone(),
            });
        }
    }

    Ok(results)
}

fn message_for_entry(entry: &clients::gcp::LogEntry) -> String {
    if let Some(payload) = &entry.json_payload {
        if let Some(msg) = payload.get("message").and_then(|v| v.as_str()) {
            return msg.to_string();
        }
    }
    if let Some(text) = &entry.text_payload {
        let first_line = text.lines().next().unwrap_or(text);
        return first_line.to_string();
    }
    "unknown error".to_string()
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
        url.push_str(&format!("&region={region}"));
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
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b => out.push_str(&format!("%{b:02X}")),
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
    use std::collections::HashMap;

    fn make_entry(
        json_payload: Option<serde_json::Value>,
        text_payload: Option<&str>,
    ) -> clients::gcp::LogEntry {
        clients::gcp::LogEntry {
            timestamp: Utc::now(),
            severity: None,
            text_payload: text_payload.map(|s| s.to_string()),
            json_payload,
            resource_labels: HashMap::new(),
            raw: String::new(),
        }
    }

    #[test]
    fn message_prefers_json_payload_message() {
        let payload = serde_json::json!({"message": "something broke", "code": 500});
        let entry = make_entry(Some(payload), Some("text fallback"));
        assert_eq!(message_for_entry(&entry), "something broke");
    }

    #[test]
    fn message_falls_back_to_text_payload_first_line() {
        let entry = make_entry(None, Some("line one\nline two"));
        assert_eq!(message_for_entry(&entry), "line one");
    }

    #[test]
    fn message_falls_back_to_unknown_error() {
        let entry = make_entry(None, None);
        assert_eq!(message_for_entry(&entry), "unknown error");
    }

    #[test]
    fn message_json_payload_without_message_key_falls_back_to_text() {
        let payload = serde_json::json!({"code": 500});
        let entry = make_entry(Some(payload), Some("text fallback"));
        assert_eq!(message_for_entry(&entry), "text fallback");
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

    #[test]
    fn lookback_to_seconds_hours() {
        assert_eq!(lookback_to_seconds("1h"), 3600);
        assert_eq!(lookback_to_seconds("24h"), 86400);
    }

    #[test]
    fn lookback_to_seconds_minutes() {
        assert_eq!(lookback_to_seconds("30m"), 1800);
    }

    #[test]
    fn lookback_to_seconds_days() {
        assert_eq!(lookback_to_seconds("7d"), 604800);
    }

    #[test]
    fn lookback_to_seconds_unknown_defaults_to_one_hour() {
        assert_eq!(lookback_to_seconds("bad"), 3600);
    }
}
