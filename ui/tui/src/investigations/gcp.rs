use domain::{InvestigationPrompt, UntrustedText};

use super::LaunchConfig;

const PROMPT: &str = include_str!("../../../../prompts/investigations/gcp.md");

#[allow(clippy::too_many_arguments)]
pub(crate) fn config(
    project: &str,
    env: &str,
    title: &str,
    message: &UntrustedText,
    line: &UntrustedText,
    url: &str,
    lookback: &str,
    gcp_project: &str,
) -> LaunchConfig {
    // expose: reading the log's own timestamp out of it, not building prompt text.
    let incident_at = serde_json::from_str::<serde_json::Value>(line.expose())
        .ok()
        .and_then(|v| {
            v.get(0)
                .and_then(|e| e.get("timestamp"))
                .and_then(|t| t.as_str())
                .map(String::from)
        })
        .unwrap_or_default();

    LaunchConfig {
        system_prompt: PROMPT.to_string(),
        prompt: InvestigationPrompt::new()
            .instruction(format!(
                "Investigate GCP error in project {project} (env: {env}, gcp_project: {gcp_project}). Title: {title}."
            ))
            .instruction("Message:")
            .untrusted("gcp log message", message)
            .instruction(format!(
                "Log lines (last {lookback}), read with the Read tool:"
            ))
            .supporting_data_path()
            .instruction(format!("Incident timestamp: {incident_at}."))
            .instruction(format!("Console URL (pre-filtered): {url}")),
        supporting_data: Some(line.clone()),
        model: "opus".to_string(),
        allowed_tools: "Bash,Read".to_string(),
        env: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::config;
    use crate::investigations::LaunchConfig;
    use domain::UntrustedText;

    /// The prompt as the agent receives it. Supplied with a path because the
    /// Loki and GCP prompts carry a supporting-data segment.
    fn rendered(cfg: &LaunchConfig) -> String {
        cfg.prompt
            .render(Some(std::path::Path::new("/tmp/supporting-data.json")))
    }

    #[test]
    fn gcp_investigation_system_prompt_contains_skill_content() {
        let cfg = config(
            "mapapp",
            "neuro",
            "errors",
            &UntrustedText::new("something broke"),
            &UntrustedText::new("{}"),
            "",
            "1h",
            "mapapp-prod-abc123",
        );
        assert!(cfg.system_prompt.contains("## Purpose"));
        assert!(!cfg.system_prompt.starts_with("---"));
    }

    #[test]
    fn gcp_investigation_prompt_contains_all_context() {
        let line = r#"[{"message":"something broke","timestamp":"2024-01-15T10:30:00Z"}]"#;
        let cfg = config(
            "mapapp",
            "neuro",
            "errors",
            &UntrustedText::new("something broke"),
            &UntrustedText::new(line),
            "https://console.cloud.google.com/logs/query",
            "1h",
            "mapapp-prod-abc123",
        );
        assert!(rendered(&cfg).contains("mapapp"));
        assert!(rendered(&cfg).contains("neuro"));
        assert!(rendered(&cfg).contains("something broke"));
        assert!(rendered(&cfg).contains("1h"));
        assert!(rendered(&cfg).contains("console.cloud.google.com"));
        assert!(rendered(&cfg).contains("mapapp-prod-abc123"));
        assert!(rendered(&cfg).contains("2024-01-15T10:30:00Z"));
        assert!(rendered(&cfg).contains("/tmp/supporting-data.json"));
        assert_eq!(
            cfg.supporting_data.as_ref().map(UntrustedText::expose),
            Some(line)
        );
    }

    #[test]
    fn gcp_investigation_incident_at_empty_when_no_timestamp() {
        let cfg = config(
            "mapapp",
            "neuro",
            "errors",
            &UntrustedText::new("something broke"),
            &UntrustedText::new(r#"[{"message":"no timestamp here"}]"#),
            "",
            "1h",
            "mapapp-prod-abc123",
        );
        assert!(rendered(&cfg).contains("Incident timestamp: ."));
    }
}
