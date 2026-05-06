use super::LaunchConfig;

pub(crate) fn config(repo: &str, run_url: &str) -> LaunchConfig {
    LaunchConfig {
        command: format!(
            "claude  --dangerously-skip-permissions '/github-ci-investigate {repo} {run_url}'"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::config;

    #[test]
    fn ci_investigation_command_passes_skill_and_context_as_one_prompt() {
        assert_eq!(
            config(
                "ooloth/hub",
                "https://github.com/ooloth/hub/actions/runs/123"
            )
            .command,
            "claude  --dangerously-skip-permissions '/github-ci-investigate ooloth/hub https://github.com/ooloth/hub/actions/runs/123'"
        );
    }
}
