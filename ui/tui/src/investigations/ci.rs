use domain::InvestigationPrompt;

use super::LaunchConfig;

const PROMPT: &str = include_str!("../../../../prompts/investigations/ci.md");

pub(crate) fn config(repo: &str, run_url: &str) -> LaunchConfig {
    LaunchConfig {
        system_prompt: PROMPT.to_string(),
        prompt: InvestigationPrompt::new().instruction(format!(
            "Investigate the CI failure for repo {repo}. Run URL: {run_url}"
        )),
        supporting_data: None,
        model: "opus".to_string(),
        allowed_tools: "Bash".to_string(),
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
        assert!(rendered(&cfg).contains("ooloth/hub"));
        assert!(rendered(&cfg).contains("https://github.com/ooloth/hub/actions/runs/123"));
    }
}
