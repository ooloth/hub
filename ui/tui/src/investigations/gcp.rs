use super::LaunchConfig;

const PROMPT: &str = include_str!("../../../../prompts/gcp-investigate.md");

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
            "Investigate GCP error in project {project} (env: {env}). \
Title: {title}. Message: {message}. \
Log lines (last {lookback}): {line}. \
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
            r#"[{"message":"something broke"}]"#,
            "https://console.cloud.google.com/logs/query",
            "1h",
        );
        assert!(cfg.prompt.contains("mapapp"));
        assert!(cfg.prompt.contains("neuro"));
        assert!(cfg.prompt.contains("something broke"));
        assert!(cfg.prompt.contains("1h"));
        assert!(cfg.prompt.contains("console.cloud.google.com"));
    }
}
