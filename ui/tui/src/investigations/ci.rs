use super::LaunchConfig;

const PROMPT: &str = include_str!("../../../../.agents/prompts/ci-investigate.md");

pub(crate) fn config(repo: &str, run_url: &str) -> LaunchConfig {
    LaunchConfig {
        system_prompt: PROMPT.to_string(),
        prompt: format!("Investigate the CI failure for repo {repo}. Run URL: {run_url}"),
        model: "opus".to_string(),
        allowed_tools: "Bash".to_string(),
        env: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::config;

    #[test]
    fn ci_investigation_system_prompt_contains_skill_content() {
        let cfg = config(
            "ooloth/hub",
            "https://github.com/ooloth/hub/actions/runs/123",
        );
        assert!(cfg.system_prompt.contains("## Purpose"));
        assert!(!cfg.system_prompt.starts_with("---"));
    }

    #[test]
    fn ci_investigation_prompt_contains_repo_and_url() {
        let cfg = config(
            "ooloth/hub",
            "https://github.com/ooloth/hub/actions/runs/123",
        );
        assert!(cfg.prompt.contains("ooloth/hub"));
        assert!(cfg
            .prompt
            .contains("https://github.com/ooloth/hub/actions/runs/123"));
    }
}
