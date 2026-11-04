use domain::{InvestigationPrompt, UntrustedText};

use super::{LaunchConfig, SupportingData};

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
    let log = serde_json::from_str::<serde_json::Value>(line.expose()).ok();
    let timestamp = log
        .as_ref()
        .and_then(|v| v.get(0))
        .and_then(|e| e.get("timestamp"))
        .and_then(|t| t.as_str());
    let incident_at = match timestamp {
        Some(t) => t.to_string(),
        None => String::new(),
    };

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
        supporting_data: Some(SupportingData {
            source: "gcp log lines".to_string(),
            body: line.clone(),
        }),
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

    /// An investigation of an error in `web-app` whose log lines are `line`.
    fn web_app_error(line: &str) -> LaunchConfig {
        config(
            "web-app",
            "neuro",
            "errors",
            &UntrustedText::new("something broke"),
            &UntrustedText::new(line),
            "https://console.cloud.google.com/logs/query",
            "1h",
            "web-app-prod-abc123",
        )
    }

    #[test]
    fn the_system_prompt_is_the_skill_without_its_front_matter() {
        let cfg = web_app_error("{}");

        assert!(cfg.system_prompt.contains("## Purpose"));
        assert!(!cfg.system_prompt.starts_with("---"));
    }

    #[test]
    fn the_prompt_carries_the_error_its_place_and_where_to_read_the_log_lines() {
        let cfg =
            web_app_error(r#"[{"message":"something broke","timestamp":"2024-01-15T10:30:00Z"}]"#);

        assert_eq!(
            rendered(&cfg),
            "Investigate GCP error in project web-app (env: neuro, gcp_project: \
             web-app-prod-abc123). Title: errors.\n\
             Message:\n\
             <untrusted-input source=\"gcp log message\">\n\
             something broke\n\
             </untrusted-input>\n\
             Log lines (last 1h), read with the Read tool:\n\
             /tmp/supporting-data.json\n\
             Incident timestamp: 2024-01-15T10:30:00Z.\n\
             Console URL (pre-filtered): https://console.cloud.google.com/logs/query"
        );
    }

    #[test]
    fn the_log_lines_are_kept_unchanged_as_supporting_data() {
        let line = r#"[{"message":"something broke","timestamp":"2024-01-15T10:30:00Z"}]"#;
        let cfg = web_app_error(line);

        assert_eq!(
            cfg.supporting_data
                .as_ref()
                // expose: asserting the config stored the line unchanged; the
                // fencing of what reaches the agent is covered in command.rs.
                .map(|data| data.body.expose()),
            Some(line)
        );
    }

    #[test]
    fn a_log_without_a_timestamp_leaves_the_incident_time_empty() {
        let cfg = web_app_error(r#"[{"message":"no timestamp here"}]"#);

        assert!(rendered(&cfg).contains("Incident timestamp: ."));
    }
}
