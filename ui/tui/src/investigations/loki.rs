use super::LaunchConfig;

const PROMPT: &str = include_str!("../../../../prompts/investigations/loki.md");

pub(crate) fn config(
    project: &str,
    env: &str,
    title: &str,
    message: &str,
    line: &str,
    url: &str,
    lookback: &str,
) -> LaunchConfig {
    LaunchConfig {
        system_prompt: PROMPT.to_string(),
        prompt: format!(
            "Investigate Loki error in project {project} (env: {env}). \
Title: {title}. Message: {message}. \
Log lines (last {lookback}): {{SUPPORTING_DATA_PATH}} — read with the Read tool. \
Grafana URL (pre-filtered): {url}"
        ),
        supporting_data: Some(line.to_string()),
        model: "opus".to_string(),
        allowed_tools: "Bash,Read".to_string(),
        env: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::config;

    #[test]
    fn loki_investigation_system_prompt_contains_skill_content() {
        let cfg = config(
            "mapapp",
            "internal",
            "backend errors",
            "Parser validation error",
            "{}",
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
            "Parser validation error",
            line,
            "https://grafana.example.com/explore",
            "15m",
        );
        assert!(cfg.prompt.contains("mapapp"));
        assert!(cfg.prompt.contains("internal"));
        assert!(cfg.prompt.contains("Parser validation error"));
        assert!(cfg.prompt.contains("15m"));
        assert!(cfg.prompt.contains("grafana.example.com"));
        assert!(cfg.prompt.contains("{SUPPORTING_DATA_PATH}"));
        assert_eq!(cfg.supporting_data.as_deref(), Some(line));
    }
}
