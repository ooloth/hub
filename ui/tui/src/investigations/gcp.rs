use super::LaunchConfig;

const PROMPT: &str = include_str!("../../../../prompts/gcp-investigate.md");

#[allow(clippy::too_many_arguments)]
pub(crate) fn config(
    project: &str,
    env: &str,
    title: &str,
    message: &str,
    line: &str,
    url: &str,
    lookback: &str,
    gcp_project: &str,
) -> LaunchConfig {
    let incident_at = serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|v| v[0]["timestamp"].as_str().map(String::from))
        .unwrap_or_default();

    LaunchConfig {
        system_prompt: PROMPT.to_string(),
        prompt: format!(
            "Investigate GCP error in project {project} (env: {env}, gcp_project: {gcp_project}). \
Title: {title}. Message: {message}. \
Log lines (last {lookback}): {line}. \
Incident timestamp: {incident_at}. \
Console URL (pre-filtered): {url}"
        ),
        model: "opus".to_string(),
        allowed_tools: "Bash,Read".to_string(),
        env: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::config;

    #[test]
    fn gcp_investigation_system_prompt_contains_skill_content() {
        let cfg = config(
            "mapapp",
            "neuro",
            "errors",
            "something broke",
            "{}",
            "",
            "1h",
            "mapapp-prod-abc123",
        );
        assert!(cfg.system_prompt.contains("## Purpose"));
        assert!(!cfg.system_prompt.starts_with("---"));
    }

    #[test]
    fn gcp_investigation_prompt_contains_all_context() {
        let cfg = config(
            "mapapp",
            "neuro",
            "errors",
            "something broke",
            r#"[{"message":"something broke","timestamp":"2024-01-15T10:30:00Z"}]"#,
            "https://console.cloud.google.com/logs/query",
            "1h",
            "mapapp-prod-abc123",
        );
        assert!(cfg.prompt.contains("mapapp"));
        assert!(cfg.prompt.contains("neuro"));
        assert!(cfg.prompt.contains("something broke"));
        assert!(cfg.prompt.contains("1h"));
        assert!(cfg.prompt.contains("console.cloud.google.com"));
        assert!(cfg.prompt.contains("mapapp-prod-abc123"));
        assert!(cfg.prompt.contains("2024-01-15T10:30:00Z"));
    }

    #[test]
    fn gcp_investigation_incident_at_empty_when_no_timestamp() {
        let cfg = config(
            "mapapp",
            "neuro",
            "errors",
            "something broke",
            r#"[{"message":"no timestamp here"}]"#,
            "",
            "1h",
            "mapapp-prod-abc123",
        );
        assert!(cfg.prompt.contains("Incident timestamp: ."));
    }
}
