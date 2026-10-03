use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::banned_terms::{self, TermList};
use super::hook_io::{deny_output, HookInput};
use super::verdict::{decide, DenyReason, Verdict};

/// Runs the guard as a hook: the list from `banned_terms`, the request from stdin, any
/// refusal to stdout. Always exits 0, because the JSON on stdout is the decision.
pub(crate) fn run(banned_terms: &Path) -> ExitCode {
    let mut stdin = String::new();
    let read = std::io::stdin().read_to_string(&mut stdin);
    let output = match (banned_terms::load(banned_terms), read) {
        (Ok(TermList::Absent), _) => None,
        (Err(error), _) => Some(deny_output(&DenyReason::Internal(format!("{error:#}")))),
        (Ok(TermList::Present(_)), Err(error)) => Some(deny_output(&DenyReason::Internal(
            format!("failed to read the hook request from stdin: {error}"),
        ))),
        (Ok(list), Ok(_)) => {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            respond(&stdin, &list, home.as_deref())
        }
    };
    if let Some(output) = output {
        println!("{output}");
    }
    ExitCode::SUCCESS
}

/// The hook's stdout for one request: `None` to allow, a deny otherwise.
///
/// The list is consulted first, so a checkout without one allows every call without
/// parsing anything.
pub(crate) fn respond(stdin: &str, list: &TermList, home: Option<&Path>) -> Option<String> {
    let TermList::Present(terms) = list else {
        return None;
    };
    let input = match HookInput::parse(stdin) {
        Ok(input) => input,
        Err(error) => return Some(deny_output(&DenyReason::Internal(format!("{error:#}")))),
    };
    let call = input.bash_call()?;
    match decide(&call.command, &call.cwd, home, terms) {
        Verdict::Allow => None,
        Verdict::Deny(reason) => Some(deny_output(&reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::super::banned_terms::BannedTerms;
    use super::*;

    fn present() -> TermList {
        TermList::Present(BannedTerms::parse("alpha"))
    }

    fn bash(command: &str) -> String {
        serde_json::json!({
            "tool_name": "Bash",
            "cwd": "/",
            "tool_input": { "command": command },
        })
        .to_string()
    }

    #[test]
    fn without_a_list_every_call_is_allowed() {
        let stdin = bash("gh issue comment 1 -b alpha");

        assert_eq!(respond(&stdin, &TermList::Absent, None), None);
    }

    #[test]
    fn without_a_list_even_malformed_input_is_allowed() {
        assert_eq!(respond("not json", &TermList::Absent, None), None);
    }

    #[test]
    fn with_a_list_malformed_input_is_refused() {
        let output = respond("not json", &present(), None).unwrap();

        assert!(output.contains("\"deny\""), "{output}");
    }

    #[test]
    fn a_post_with_a_banned_term_is_refused() {
        let output = respond(&bash("gh issue comment 1 -b alpha"), &present(), None).unwrap();

        assert!(output.contains("\"deny\""), "{output}");
    }

    #[test]
    fn a_clean_post_is_allowed() {
        assert_eq!(
            respond(&bash("gh issue comment 1 -b fine"), &present(), None),
            None
        );
    }

    #[test]
    fn another_tool_is_allowed() {
        let stdin = r#"{"tool_name":"Write","cwd":"/","tool_input":{"content":"alpha"}}"#;

        assert_eq!(respond(stdin, &present(), None), None);
    }
}
