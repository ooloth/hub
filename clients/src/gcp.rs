use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// A single structured log entry returned by `gcloud logging read`.
#[derive(Debug)]
pub struct LogEntry {
    /// RFC 3339 timestamp of the log entry.
    pub timestamp: DateTime<Utc>,
    /// Severity level (e.g. `"ERROR"`, `"WARNING"`).
    pub severity: Option<String>,
    /// Free-text payload, if present.
    pub text_payload: Option<String>,
    /// Structured JSON payload, if present.
    pub json_payload: Option<serde_json::Value>,
    /// HTTP request metadata, if present.
    pub http_request: Option<serde_json::Value>,
    /// Key-value labels from the log entry's `resource.labels` field.
    pub resource_labels: HashMap<String, String>,
    /// Raw JSON representation of the full log entry.
    pub raw: String,
}

/// # Errors
/// Returns an error if `gcloud` is not installed, exits non-zero, or its
/// output cannot be parsed as JSON log entries.
pub async fn entries(gcp_project: &str, filter: &str, lookback: &str) -> Result<Vec<LogEntry>> {
    let args = gcloud_args(gcp_project, filter, lookback);
    let output = tokio::process::Command::new("gcloud")
        .args(&args)
        .output()
        .await
        .context("failed to run gcloud — is it installed and authenticated?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gcloud exited with status {}: {stderr}", output.status);
    }

    let json = String::from_utf8_lossy(&output.stdout);
    parse_entries(&json)
}

fn gcloud_args(gcp_project: &str, filter: &str, lookback: &str) -> Vec<String> {
    vec![
        "logging".into(),
        "read".into(),
        filter.into(),
        format!("--project={gcp_project}"),
        format!("--freshness={lookback}"),
        "--format=json".into(),
        "--limit=1000".into(),
    ]
}

fn parse_entries(json: &str) -> Result<Vec<LogEntry>> {
    let values: Vec<serde_json::Value> =
        serde_json::from_str(json).context("gcloud output was not valid JSON")?;

    let entries = values
        .into_iter()
        .map(|v| {
            let raw = v.to_string();

            let timestamp = v
                .get("timestamp")
                .and_then(|t| t.as_str())
                .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                .unwrap_or_else(Utc::now);

            let severity = v
                .get("severity")
                .and_then(|s| s.as_str())
                .map(std::string::ToString::to_string);

            let text_payload = v
                .get("textPayload")
                .and_then(|t| t.as_str())
                .map(std::string::ToString::to_string);

            let json_payload = v.get("jsonPayload").cloned();

            let http_request = v.get("httpRequest").cloned();

            let resource_labels = v
                .get("resource")
                .and_then(|r| r.get("labels"))
                .and_then(|l| l.as_object())
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();

            LogEntry {
                timestamp,
                severity,
                text_payload,
                json_payload,
                http_request,
                resource_labels,
                raw,
            }
        })
        .collect();

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("my-project", "severity>=ERROR", "1h", vec![
        "logging", "read", "severity>=ERROR",
        "--project=my-project", "--freshness=1h", "--format=json", "--limit=1000",
    ])]
    fn gcloud_args_produces_correct_args(
        #[case] project: &str,
        #[case] filter: &str,
        #[case] lookback: &str,
        #[case] expected: Vec<&str>,
    ) {
        let got = gcloud_args(project, filter, lookback);
        assert_eq!(got, expected);
    }

    #[rstest]
    #[case("1h")]
    #[case("30m")]
    #[case("24h")]
    #[case("7d")]
    fn gcloud_args_passes_lookback_through_unchanged(#[case] lookback: &str) {
        let args = gcloud_args("proj", "filter", lookback);
        assert!(
            args.contains(&format!("--freshness={lookback}")),
            "expected --freshness={lookback} in {args:?}"
        );
    }

    #[test]
    fn parse_entries_empty_array_returns_empty_vec() {
        let entries = parse_entries("[]").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_entries_invalid_json_returns_error() {
        assert!(parse_entries("not json").is_err());
    }

    #[test]
    fn parse_entries_json_payload_entry() {
        let json = r#"[{
            "timestamp": "2024-01-15T10:30:00Z",
            "severity": "ERROR",
            "jsonPayload": {"message": "something broke", "code": 500}
        }]"#;
        let entries = parse_entries(json).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.severity.as_deref(), Some("ERROR"));
        assert!(e.json_payload.is_some());
        assert!(e.text_payload.is_none());
        assert_eq!(
            e.json_payload
                .as_ref()
                .unwrap()
                .get("message")
                .and_then(|v| v.as_str()),
            Some("something broke")
        );
    }

    #[test]
    fn parse_entries_text_payload_entry() {
        let json = r#"[{
            "timestamp": "2024-01-15T10:30:00Z",
            "severity": "WARNING",
            "textPayload": "disk usage above 90%"
        }]"#;
        let entries = parse_entries(json).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.severity.as_deref(), Some("WARNING"));
        assert_eq!(e.text_payload.as_deref(), Some("disk usage above 90%"));
        assert!(e.json_payload.is_none());
    }

    #[test]
    fn parse_entries_missing_timestamp_uses_now() {
        let json = r#"[{"severity": "ERROR"}]"#;
        let before = Utc::now();
        let entries = parse_entries(json).unwrap();
        let after = Utc::now();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].timestamp >= before);
        assert!(entries[0].timestamp <= after);
    }

    #[test]
    fn parse_entries_raw_field_is_json_representation() {
        let json = r#"[{"timestamp": "2024-01-15T10:30:00Z", "severity": "ERROR"}]"#;
        let entries = parse_entries(json).unwrap();
        assert_eq!(entries.len(), 1);
        let raw: serde_json::Value = serde_json::from_str(&entries[0].raw).unwrap();
        assert_eq!(raw.get("severity").and_then(|v| v.as_str()), Some("ERROR"));
    }

    #[test]
    fn parse_entries_http_request_entry() {
        let json = r#"[{
            "timestamp": "2024-01-15T10:30:00Z",
            "severity": "ERROR",
            "httpRequest": {"status": 503, "requestUrl": "https://example.com/ingest", "latency": "54s"}
        }]"#;
        let entries = parse_entries(json).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert!(e.json_payload.is_none());
        assert!(e.text_payload.is_none());
        assert_eq!(
            e.http_request
                .as_ref()
                .unwrap()
                .get("status")
                .and_then(|v| v.as_u64()),
            Some(503)
        );
    }

    #[test]
    fn parse_entries_resource_labels_extracted() {
        let json = r#"[{
            "timestamp": "2024-01-15T10:30:00Z",
            "resource": {
                "type": "cloud_run_revision",
                "labels": {"service_name": "my-service", "revision_name": "my-service-00042-abc"}
            }
        }]"#;
        let entries = parse_entries(json).unwrap();
        assert_eq!(
            entries[0]
                .resource_labels
                .get("service_name")
                .map(|s| s.as_str()),
            Some("my-service")
        );
    }
}
