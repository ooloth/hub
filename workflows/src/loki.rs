use anyhow::Result;
use chrono::Utc;
use domain::{LokiEntry, LokiEnv, Urgency};

pub async fn run(env: &LokiEnv) -> Result<Vec<LokiEntry>> {
    let mut results = Vec::new();

    for query in &env.queries {
        let entries = clients::loki::entries(
            &env.endpoint,
            env.token.as_deref(),
            &query.query,
            &query.lookback,
        )
        .await?;

        let url = grafana_explore_url(env.grafana_url.as_deref(), &query.query, &query.lookback);

        for entry in &entries {
            let message = entry
                .labels
                .get("message")
                .cloned()
                .unwrap_or_else(|| "unknown error".to_string());

            results.push(LokiEntry {
                title: query.title.clone(),
                project: env.project.clone(),
                env: env.env.clone(),
                message,
                line: entry.line.clone(),
                lookback: query.lookback.clone(),
                age: age_from_entry(entry),
                urgency: Urgency::High,
                url: url.clone(),
            });
        }
    }

    Ok(results)
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
