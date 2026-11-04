use domain::{InvestigationPrompt, UntrustedText};

use super::{LaunchConfig, SupportingData};

const PROMPT: &str = include_str!("../../../../prompts/investigations/loki.md");

pub(crate) fn config(
    project: &str,
    env: &str,
    title: &str,
    message: &UntrustedText,
    line: &UntrustedText,
    url: &str,
    lookback: &str,
) -> LaunchConfig {
    LaunchConfig {
        system_prompt: PROMPT.to_string(),
        prompt: InvestigationPrompt::new()
            .instruction(format!(
                "Investigate Loki error in project {project} (env: {env}). Title: {title}."
            ))
            .instruction("Message:")
            .untrusted("loki log message", message)
            .instruction(format!(
                "Log lines (last {lookback}), read with the Read tool:"
            ))
            .supporting_data_path()
            .instruction(format!("Grafana URL (pre-filtered): {url}")),
        supporting_data: Some(SupportingData {
            source: "loki log lines".to_string(),
            body: line.clone(),
        }),
        model: "opus".to_string(),
        allowed_tools: "Bash,Read".to_string(),
        env: vec![],
    }
}

// The prompt and system prompt are pinned by the snapshots in `command.rs`.
#[cfg(test)]
mod tests {
    use super::config;
    use domain::UntrustedText;

    #[test]
    fn the_log_line_is_kept_unchanged_as_supporting_data() {
        let line = r#"[{"message":"Parser validation error"}]"#;
        let cfg = config(
            "web-app",
            "internal",
            "backend errors",
            &UntrustedText::new("Parser validation error"),
            &UntrustedText::new(line),
            "https://grafana.example.com/explore",
            "15m",
        );
        assert_eq!(
            cfg.supporting_data
                .as_ref()
                // expose: asserting the config stored the line unchanged; the
                // fencing of what reaches the agent is covered in command.rs.
                .map(|data| data.body.expose()),
            Some(line)
        );
    }
}
