use anyhow::Result;
use std::path::Path;

use super::launch_in_tmux_split;

pub(crate) fn launch(repo: &str, run_url: &str, cwd: &Path) -> Result<()> {
    launch_in_tmux_split(&command(repo, run_url), cwd)
}

fn command(repo: &str, run_url: &str) -> String {
    format!("claude  --dangerously-skip-permissions '/github-ci-investigate {repo} {run_url}'")
}

#[cfg(test)]
mod tests {
    use super::command;

    #[test]
    fn ci_investigation_command_passes_skill_and_context_as_one_prompt() {
        assert_eq!(
            command(
                "ooloth/hub",
                "https://github.com/ooloth/hub/actions/runs/123"
            ),
            "claude  --dangerously-skip-permissions '/github-ci-investigate ooloth/hub https://github.com/ooloth/hub/actions/runs/123'"
        );
    }
}
