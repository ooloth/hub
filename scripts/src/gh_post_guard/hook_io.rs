//! The Claude Code hook protocol: the request on stdin and the decision on stdout.
//! See https://code.claude.com/docs/en/hooks for both shapes.
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::verdict::DenyReason;

/// The `PreToolUse` request Claude Code sends on stdin. Only the fields the guard reads.
#[derive(Debug, Deserialize)]
pub(crate) struct HookInput {
    tool_name: String,
    #[serde(default)]
    tool_input: serde_json::Value,
    cwd: PathBuf,
}

/// A Bash tool call: the command it would run and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BashCall {
    pub(crate) command: String,
    pub(crate) cwd: PathBuf,
}

impl HookInput {
    /// Parses the hook's stdin.
    ///
    /// # Errors
    /// Returns an error when stdin is not the hook's JSON.
    pub(crate) fn parse(stdin: &str) -> Result<Self> {
        serde_json::from_str(stdin).context("stdin is not a PreToolUse hook request")
    }

    /// The Bash call this request is about, or `None` for any other tool.
    pub(crate) fn bash_call(self) -> Option<BashCall> {
        if self.tool_name != "Bash" {
            return None;
        }
        let command = self.tool_input.get("command")?.as_str()?.to_string();
        Some(BashCall {
            command,
            cwd: self.cwd,
        })
    }
}

/// The hook's stdout for a refusal: a `PreToolUse` deny carrying the reason.
///
/// A JSON deny blocks the call in every permission mode, including bypass, which is
/// why the guard answers this way rather than with an exit code.
pub(crate) fn deny_output(reason: &DenyReason) -> String {
    let reason = reason.to_string();
    assert!(!reason.is_empty(), "a refusal must say why");
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bash_request_yields_its_command_and_directory() {
        let stdin = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","cwd":"/work",
            "tool_input":{"command":"gh issue view 1","description":"x"}}"#;

        let call = HookInput::parse(stdin).unwrap().bash_call();

        assert_eq!(
            call,
            Some(BashCall {
                command: "gh issue view 1".to_string(),
                cwd: PathBuf::from("/work"),
            })
        );
    }

    #[test]
    fn another_tool_yields_no_bash_call() {
        let stdin = r#"{"tool_name":"Edit","cwd":"/work","tool_input":{"file_path":"x"}}"#;

        assert_eq!(HookInput::parse(stdin).unwrap().bash_call(), None);
    }

    #[test]
    fn stdin_that_is_not_hook_json_is_an_error() {
        assert!(HookInput::parse("not json").is_err());
    }

    #[test]
    fn a_refusal_is_a_pre_tool_use_deny_with_its_reason() {
        let output = deny_output(&DenyReason::UnparseableCommand);

        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        let decision = &json["hookSpecificOutput"];
        assert_eq!(decision["hookEventName"], "PreToolUse");
        assert_eq!(decision["permissionDecision"], "deny");
        assert_eq!(
            decision["permissionDecisionReason"],
            DenyReason::UnparseableCommand.to_string()
        );
    }
}
