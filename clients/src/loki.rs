use anyhow::{Context, Result};
use chrono::Utc;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct QueryRangeResponse {
    data: QueryRangeData,
}

#[derive(Deserialize)]
struct QueryRangeData {
    result: Vec<Stream>,
}

#[derive(Deserialize)]
struct Stream {
    stream: HashMap<String, String>,
    values: Vec<(String, String)>,
}

/// A single log entry returned by the Loki API.
#[derive(Debug, PartialEq)]
pub struct LogEntry {
    pub timestamp_ns: i64,
    pub line: String,
    pub labels: HashMap<String, String>,
}

/// Returns log entries for the given LogQL query over the lookback window.
///
/// # Errors
/// Returns an error if the Loki API is unreachable, returns a non-2xx response,
/// or if `lookback` is not a valid duration string (e.g. "1h", "30m").
pub async fn entries(
    endpoint: &str,
    token: Option<&str>,
    query: &str,
    lookback: &str,
) -> Result<Vec<LogEntry>> {
    let duration = humantime::parse_duration(lookback)
        .with_context(|| format!("invalid lookback: {lookback}"))?;

    let now_ns = Utc::now()
        .timestamp_nanos_opt()
        .context("system clock out of range")?;
    let lookback_ns = i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX);
    let start_ns = now_ns.saturating_sub(lookback_ns);

    let mut request = reqwest::Client::new()
        .get(format!("{endpoint}/loki/api/v1/query_range"))
        .query(&[
            ("query", query),
            ("start", &start_ns.to_string()),
            ("end", &now_ns.to_string()),
            ("limit", "1000"),
            ("direction", "BACKWARD"),
        ])
        .header("User-Agent", "hub-cli");

    if let Some(t) = token {
        request = request.bearer_auth(t);
    }

    let response: QueryRangeResponse = request
        .send()
        .await
        .context("failed to reach Loki API")?
        .error_for_status()
        .context("Loki API returned an error")?
        .json()
        .await
        .context("failed to parse Loki response")?;

    Ok(entries_from_response(response))
}

fn entries_from_response(response: QueryRangeResponse) -> Vec<LogEntry> {
    response
        .data
        .result
        .into_iter()
        .flat_map(|stream| {
            let labels = stream.stream;
            stream.values.into_iter().map(move |(ts, line)| LogEntry {
                timestamp_ns: ts.parse().unwrap_or(0),
                line,
                labels: labels.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stream(labels: &[(&str, &str)], values: &[(&str, &str)]) -> Stream {
        Stream {
            stream: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            values: values
                .iter()
                .map(|(ts, line)| (ts.to_string(), line.to_string()))
                .collect(),
        }
    }

    fn make_response(streams: Vec<Stream>) -> QueryRangeResponse {
        QueryRangeResponse {
            data: QueryRangeData { result: streams },
        }
    }

    #[test]
    fn extracts_timestamp_line_and_labels() {
        let stream = make_stream(
            &[("app", "myapp"), ("env", "prod")],
            &[("1000000000", "error: something failed")],
        );
        let entries = entries_from_response(make_response(vec![stream]));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].timestamp_ns, 1_000_000_000);
        assert_eq!(entries[0].line, "error: something failed");
        assert_eq!(entries[0].labels.get("app"), Some(&"myapp".to_string()));
        assert_eq!(entries[0].labels.get("env"), Some(&"prod".to_string()));
    }

    #[test]
    fn returns_empty_for_no_streams() {
        assert!(entries_from_response(make_response(vec![])).is_empty());
    }

    #[test]
    fn each_entry_carries_its_streams_labels() {
        let stream_a = make_stream(&[("app", "a")], &[("1", "line from a")]);
        let stream_b = make_stream(&[("app", "b")], &[("2", "line from b")]);
        let entries = entries_from_response(make_response(vec![stream_a, stream_b]));
        assert_eq!(entries.len(), 2);
        let a = entries.iter().find(|e| e.line == "line from a").unwrap();
        let b = entries.iter().find(|e| e.line == "line from b").unwrap();
        assert_eq!(a.labels.get("app"), Some(&"a".to_string()));
        assert_eq!(b.labels.get("app"), Some(&"b".to_string()));
    }

    #[test]
    fn multiple_entries_in_one_stream_all_share_labels() {
        let stream = make_stream(&[("app", "myapp")], &[("1", "first"), ("2", "second")]);
        let entries = entries_from_response(make_response(vec![stream]));
        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .all(|e| e.labels.get("app") == Some(&"myapp".to_string())));
    }

    #[test]
    fn stream_with_no_labels_yields_empty_label_map() {
        let stream = make_stream(&[], &[("1", "unlabeled line")]);
        let entries = entries_from_response(make_response(vec![stream]));
        assert_eq!(entries.len(), 1);
        assert!(entries[0].labels.is_empty());
    }

    #[test]
    fn accepts_valid_lookback_durations() {
        assert!(humantime::parse_duration("1h").is_ok());
        assert!(humantime::parse_duration("30m").is_ok());
        assert!(humantime::parse_duration("7d").is_ok());
    }

    #[test]
    fn rejects_invalid_lookback() {
        assert!(humantime::parse_duration("not-a-duration").is_err());
    }
}
