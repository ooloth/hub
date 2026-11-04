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
