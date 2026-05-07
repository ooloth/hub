use anyhow::Result;
use chrono::Utc;
use domain::{LokiEnv, LokiErrors, Urgency};

pub async fn run(env: &LokiEnv) -> Result<Vec<LokiErrors>> {
    let mut results = Vec::new();

    for query in &env.queries {
        let entries = clients::loki::entries(
            &env.endpoint,
            env.token.as_deref(),
            &query.query,
            &query.lookback,
        )
        .await?;

        let count = u32::try_from(entries.len()).unwrap_or(u32::MAX);
        if count >= query.threshold {
            results.push(LokiErrors {
                title: query.title.clone(),
                project: env.project.clone(),
                env: env.env.clone(),
                error_count: count,
                threshold: query.threshold,
                lookback: query.lookback.clone(),
                age: age_from_entries(&entries),
                urgency: Urgency::High,
            });
        }
    }

    Ok(results)
}

fn age_from_entries(entries: &[clients::loki::LogEntry]) -> chrono::Duration {
    // entries is non-empty when called (count >= threshold >= 1)
    let oldest_ns = entries.last().expect("non-empty").timestamp_ns;
    let now_ns = Utc::now().timestamp_nanos_opt().unwrap_or(oldest_ns);
    let age_secs = ((now_ns - oldest_ns) / 1_000_000_000).max(0);
    chrono::Duration::seconds(age_secs)
}
