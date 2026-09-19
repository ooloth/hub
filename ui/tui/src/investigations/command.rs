//! The tmux invocation an investigation turns into, built without running it.
//!
//! `launch()` used to resolve a worktree, write a temp file and assemble the
//! command in one async function, so the only way to see what it produced was
//! to spawn a process. Everything here is pure: `cwd` and the supporting-data
//! path arrive as arguments rather than being computed, which is what lets a
//! test pin the whole invocation.

use std::path::{Path, PathBuf};

use super::LaunchConfig;

/// The placeholder a prompt uses to refer to its supporting-data file.
///
/// `launch()` writes the data before the path exists, so the prompt is written
/// against this and the path is substituted here.
pub(crate) const SUPPORTING_DATA_PLACEHOLDER: &str = "{SUPPORTING_DATA_PATH}";

/// Everything `tmux new-window` needs for one investigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InvestigationCommand {
    /// Directory the window starts in.
    pub(crate) cwd: PathBuf,
    /// Variables tmux sets on the window, in the order they are passed.
    pub(crate) env: Vec<(String, String)>,
    /// The shell command tmux runs.
    pub(crate) shell: String,
}

/// Builds the invocation for an investigation, running nothing.
///
/// Prompts travel in the environment rather than in `shell`. The command
/// references them as `"$HUB_SYSTEM_PROMPT"` and `"$HUB_TASK_PROMPT"`, and tmux
/// receives each value as its own argv element, so nothing in a prompt is
/// re-evaluated as shell syntax. Keeping that true is the reason this function
/// exists as a value-returning one.
///
/// # Panics
/// Panics if supporting data was written without the prompt referring to it, or
/// the reverse, and if an environment key is empty or contains `=`.
pub(crate) fn compose(
    config: LaunchConfig,
    cwd: &Path,
    cleanup: Option<&str>,
    supporting_data_path: Option<&Path>,
) -> InvestigationCommand {
    let placeholder_present = config.prompt.contains(SUPPORTING_DATA_PLACEHOLDER);
    if supporting_data_path.is_some() {
        assert!(
            placeholder_present,
            "supporting data was written but the prompt has no {SUPPORTING_DATA_PLACEHOLDER} pointing at it"
        );
    }
    if placeholder_present {
        assert!(
            supporting_data_path.is_some(),
            "the prompt references {SUPPORTING_DATA_PLACEHOLDER} but no supporting data was written"
        );
    }

    let prompt = match supporting_data_path {
        Some(path) => config
            .prompt
            .replace(SUPPORTING_DATA_PLACEHOLDER, &path.to_string_lossy()),
        None => config.prompt,
    };

    let mut env = vec![
        ("HUB_SYSTEM_PROMPT".to_string(), config.system_prompt),
        ("HUB_TASK_PROMPT".to_string(), prompt.clone()),
    ];
    env.extend(config.env);

    for (key, _) in &env {
        assert!(!key.is_empty(), "tmux environment key must not be empty");
        assert!(
            !key.contains('='),
            "tmux environment key must not contain '=', got {key}"
        );
    }

    let task_arg = if prompt.is_empty() {
        String::new()
    } else {
        " \"$HUB_TASK_PROMPT\"".to_string()
    };
    let cleanup_suffix = cleanup.map(|c| format!("; {c}")).unwrap_or_default();
    let shell = format!(
        "claude --dangerously-skip-permissions --model {} --allowedTools '{}' --append-system-prompt \"$HUB_SYSTEM_PROMPT\"{task_arg}{cleanup_suffix}",
        config.model, config.allowed_tools,
    );

    InvestigationCommand {
        cwd: cwd.to_path_buf(),
        env,
        shell,
    }
}

#[cfg(test)]
mod tests {
    use super::{compose, InvestigationCommand, SUPPORTING_DATA_PLACEHOLDER};
    use crate::investigations::{ci, gcp, issue, loki, pr, LaunchConfig};
    use crate::state::{PrAuthor, PrReview, PrReviewTarget};
    use std::path::Path;

    fn cwd() -> &'static Path {
        Path::new("/tmp/hub-test-worktree")
    }

    fn data_path() -> &'static Path {
        Path::new("/tmp/hub-supporting-data-000.json")
    }

    const CLEANUP: &str =
        "cd ~ && git -C '/tmp/bare' worktree remove --force '/tmp/wt' 2>/dev/null || true";

    /// A config with nothing externally-sourced in it, for the cases that are
    /// about the shape of the invocation rather than about a signal type.
    fn plain(prompt: &str) -> LaunchConfig {
        LaunchConfig {
            system_prompt: "## Purpose\n\nDo the thing.".to_string(),
            prompt: prompt.to_string(),
            supporting_data: None,
            model: "opus".to_string(),
            allowed_tools: "Bash,Read".to_string(),
            env: vec![],
        }
    }

    fn env_value(command: &InvestigationCommand, key: &str) -> String {
        command
            .env
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("no {key} in environment"))
    }

    /// Renders a command for snapshotting.
    ///
    /// `HUB_SYSTEM_PROMPT` is reduced to its first line. Its full text is a
    /// whole file from `prompts/investigations/`, so including it would churn
    /// every snapshot here whenever a prompt is edited, and the prompt files
    /// have their own coverage. The task prompt is rendered in full: it is
    /// where signal text ends up, so it is the part worth pinning.
    fn render(command: &InvestigationCommand) -> String {
        let mut out = format!("cwd: {}\n", command.cwd.display());
        out.push_str("env:\n");
        for (key, value) in &command.env {
            if key == "HUB_SYSTEM_PROMPT" {
                let first = value.lines().next().unwrap_or_default();
                out.push_str(&format!("  {key}=<system prompt starting {first:?}>\n"));
            } else {
                out.push_str(&format!("  {key}={value}\n"));
            }
        }
        out.push_str(&format!("shell: {}\n", command.shell));
        out
    }

    #[test]
    fn an_empty_task_prompt_produces_no_positional_argument() {
        let command = compose(plain(""), cwd(), None, None);
        assert!(
            !command.shell.contains("$HUB_TASK_PROMPT"),
            "shell referenced an empty task prompt: {}",
            command.shell
        );
    }

    #[test]
    fn a_task_prompt_is_referenced_by_the_shell_when_present() {
        let command = compose(plain("Investigate this"), cwd(), None, None);
        assert!(command.shell.ends_with(" \"$HUB_TASK_PROMPT\""));
    }

    /// The property the whole issue rests on: signal text travels in the
    /// environment, so no part of it is ever shell syntax.
    #[test]
    fn shell_syntax_in_a_prompt_stays_out_of_the_shell_string() {
        let hostile = "Message: $(touch /tmp/hub-injection-probe) `id` \"quoted\"";
        let command = compose(plain(hostile), cwd(), None, None);
        assert!(
            !command.shell.contains("touch"),
            "prompt text leaked into the shell string: {}",
            command.shell
        );
        assert_eq!(env_value(&command, "HUB_TASK_PROMPT"), hostile);
    }

    #[test]
    fn the_supporting_data_placeholder_becomes_the_written_path() {
        let command = compose(
            plain(&format!("Read {SUPPORTING_DATA_PLACEHOLDER} for context")),
            cwd(),
            None,
            Some(data_path()),
        );
        assert_eq!(
            env_value(&command, "HUB_TASK_PROMPT"),
            "Read /tmp/hub-supporting-data-000.json for context"
        );
    }

    #[test]
    fn the_cleanup_command_is_appended_to_the_shell_string() {
        let command = compose(plain("Investigate"), cwd(), Some(CLEANUP), None);
        assert!(command.shell.ends_with(&format!("; {CLEANUP}")));
    }

    #[test]
    fn config_environment_is_passed_through_alongside_the_prompts() {
        let mut config = plain("Investigate");
        config.env = vec![("SOME_URL".to_string(), "https://example.com".to_string())];
        let command = compose(config, cwd(), None, None);
        assert_eq!(env_value(&command, "SOME_URL"), "https://example.com");
    }

    #[test]
    #[should_panic(expected = "supporting data was written but the prompt has no")]
    fn writing_supporting_data_no_prompt_points_at_is_a_bug() {
        let _ = compose(plain("Investigate"), cwd(), None, Some(data_path()));
    }

    #[test]
    #[should_panic(expected = "but no supporting data was written")]
    fn referencing_supporting_data_that_was_never_written_is_a_bug() {
        let _ = compose(
            plain(&format!("Read {SUPPORTING_DATA_PLACEHOLDER}")),
            cwd(),
            None,
            None,
        );
    }

    #[test]
    #[should_panic(expected = "must not be empty")]
    fn an_empty_environment_key_is_a_bug() {
        let mut config = plain("Investigate");
        config.env = vec![(String::new(), "value".to_string())];
        let _ = compose(config, cwd(), None, None);
    }

    /// `tmux -e A=B=C` reads the key as `A` and the value as `B=C`, so a key
    /// carrying a `=` silently sets something other than what was asked for.
    #[test]
    #[should_panic(expected = "must not contain '='")]
    fn an_environment_key_containing_an_equals_sign_is_a_bug() {
        let mut config = plain("Investigate");
        config.env = vec![("A=B".to_string(), "value".to_string())];
        let _ = compose(config, cwd(), None, None);
    }

    // ── One snapshot per investigation type ───────────────────────────────────

    #[test]
    fn ci_investigation_command() {
        let config = ci::config(
            "ooloth/hub",
            "https://github.com/ooloth/hub/actions/runs/123",
        );
        insta::assert_snapshot!(render(&compose(config, cwd(), Some(CLEANUP), None)));
    }

    #[test]
    fn issue_investigation_command() {
        let config = issue::config("ooloth/hub", 42);
        insta::assert_snapshot!(render(&compose(config, cwd(), Some(CLEANUP), None)));
    }

    #[test]
    fn loki_investigation_command() {
        let config = loki::config(
            "mapapp",
            "internal",
            "backend errors",
            "Parser validation error",
            r#"[{"message":"Parser validation error"}]"#,
            "https://grafana.example.com/explore",
            "15m",
        );
        insta::assert_snapshot!(render(&compose(
            config,
            cwd(),
            Some(CLEANUP),
            Some(data_path())
        )));
    }

    #[test]
    fn gcp_investigation_command() {
        let config = gcp::config(
            "mapapp",
            "internal",
            "backend errors",
            "Parser validation error",
            r#"[{"timestamp":"2026-01-01T00:00:00Z","message":"Parser validation error"}]"#,
            "https://console.cloud.google.com/logs",
            "15m",
            "my-gcp-project",
        );
        insta::assert_snapshot!(render(&compose(
            config,
            cwd(),
            Some(CLEANUP),
            Some(data_path())
        )));
    }

    #[test]
    fn pr_ask_investigation_command() {
        let config = pr::ask_config(7, "ooloth/hub", PrAuthor::Peer);
        insta::assert_snapshot!(render(&compose(config, cwd(), None, None)));
    }

    #[test]
    fn pr_review_investigation_command() {
        let target = PrReviewTarget {
            repo: "ooloth/hub".to_string(),
            number: 7,
            head_branch: "feature".to_string(),
            author: PrAuthor::Me,
        };
        let config = pr::review_config(&target, PrReview::ReviewMine);
        insta::assert_snapshot!(render(&compose(config, cwd(), None, None)));
    }
}
