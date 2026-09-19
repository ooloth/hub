use domain::{InvestigationPrompt, UntrustedText};

use super::LaunchConfig;

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
    fn loki_investigation_system_prompt_contains_skill_content() {
        let cfg = config(
            "mapapp",
            "internal",
            "backend errors",
            &UntrustedText::new("Parser validation error"),
            &UntrustedText::new("{}"),
            "",
            "15m",
        );
        assert!(cfg.system_prompt.contains("## Purpose"));
        assert!(!cfg.system_prompt.starts_with("---"));
    }

    #[test]
    fn loki_investigation_prompt_contains_all_context() {
        let line = r#"[{"message":"Parser validation error"}]"#;
        let cfg = config(
            "mapapp",
            "internal",
            "backend errors",
            &UntrustedText::new("Parser validation error"),
            &UntrustedText::new(line),
            "https://grafana.example.com/explore",
            "15m",
        );
        assert!(rendered(&cfg).contains("mapapp"));
        assert!(rendered(&cfg).contains("internal"));
        assert!(rendered(&cfg).contains("Parser validation error"));
        assert!(rendered(&cfg).contains("15m"));
        assert!(rendered(&cfg).contains("grafana.example.com"));
        assert!(rendered(&cfg).contains("/tmp/supporting-data.json"));
        assert_eq!(
            cfg.supporting_data.as_ref().map(UntrustedText::expose),
            Some(line)
        );
    }
}
