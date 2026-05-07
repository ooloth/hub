use super::LaunchConfig;

pub(crate) fn config(
    project: &str,
    env: &str,
    title: &str,
    message: &str,
    line: &str,
) -> LaunchConfig {
    LaunchConfig {
        command: format!(
            "claude --dangerously-skip-permissions '/loki-investigate {project} {env} {title} {message}' --context '{line}'"
        ),
        env: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::config;

    #[test]
    fn loki_investigation_command_includes_all_context() {
        let cfg = config(
            "mapapp",
            "internal",
            "backend errors",
            "Parser validation error",
            r#"{"message":"Parser validation error","severity":"error"}"#,
        );
        assert!(cfg.command.contains("/loki-investigate"));
        assert!(cfg.command.contains("mapapp"));
        assert!(cfg.command.contains("internal"));
        assert!(cfg.command.contains("Parser validation error"));
    }
}
