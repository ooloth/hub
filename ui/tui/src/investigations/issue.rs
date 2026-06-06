use super::LaunchConfig;

const PROMPT: &str = include_str!("../../../../prompts/investigations/issue.md");

pub(crate) fn config(repo: &str, number: u64) -> LaunchConfig {
    LaunchConfig {
        system_prompt: PROMPT.to_string(),
        prompt: format!("Investigate GitHub issue #{number} in repo {repo}"),
        supporting_data: None,
        model: "opus".to_string(),
        allowed_tools: "Bash,Read".to_string(),
        env: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::config;

    #[test]
    fn issue_investigation_system_prompt_contains_skill_content() {
        let cfg = config("ooloth/hub", 42);
        assert!(cfg.system_prompt.contains("## Purpose"));
        assert!(!cfg.system_prompt.starts_with("---"));
    }

    #[test]
    fn issue_investigation_prompt_contains_repo_and_number() {
        let cfg = config("ooloth/hub", 42);
        assert!(cfg.prompt.contains("ooloth/hub"));
        assert!(cfg.prompt.contains("42"));
    }
}
