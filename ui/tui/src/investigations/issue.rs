use domain::InvestigationPrompt;

use super::LaunchConfig;

const PROMPT: &str = include_str!("../../../../prompts/investigations/issue.md");

pub(crate) fn config(repo: &str, number: u64) -> LaunchConfig {
    LaunchConfig {
        system_prompt: PROMPT.to_string(),
        prompt: InvestigationPrompt::new()
            .instruction(format!("Investigate GitHub issue #{number} in repo {repo}")),
        supporting_data: None,
        model: "opus".to_string(),
        allowed_tools: "Bash,Read".to_string(),
        env: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::config;
    use crate::investigations::LaunchConfig;

    /// The prompt as the agent receives it. Supplied with a path because the
    /// Loki and GCP prompts carry a supporting-data segment.
    fn rendered(cfg: &LaunchConfig) -> String {
        cfg.prompt
            .render(Some(std::path::Path::new("/tmp/supporting-data.json")))
    }

    #[test]
    fn issue_investigation_system_prompt_contains_skill_content() {
        let cfg = config("ooloth/hub", 42);
        assert!(cfg.system_prompt.contains("## Purpose"));
        assert!(!cfg.system_prompt.starts_with("---"));
    }

    #[test]
    fn issue_investigation_prompt_contains_repo_and_number() {
        let cfg = config("ooloth/hub", 42);
        assert!(rendered(&cfg).contains("ooloth/hub"));
        assert!(rendered(&cfg).contains("42"));
    }
}
