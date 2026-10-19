//! Claude Code session transcript parsing.
//!
//! Types and functions for reading agent session JSONL files written by Claude Code
//! to `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`. These are used by the
//! TUI's stream view to display an in-progress or completed agent session.
//!
//! For the session *state* file (`~/.claude/sessions/<pid>.json`) used for completion
//! and stall detection, see `workflows::agent_session`.

/// One block of content from an agent session transcript.
#[derive(Clone, Debug)]
pub enum StreamBlock {
    /// A message sent by the human (the opening prompt or a follow-up).
    HumanTurn(String),
    /// A prose response from the assistant.
    AssistantText(String),
    /// An extended-thinking block from the assistant.
    AssistantThinking(String),
    /// A tool invocation by the assistant, with a short human-readable summary.
    ToolCall {
        /// Tool name (e.g. "Read", "Bash").
        name: String,
        /// Human-readable summary of the invocation (e.g. the command or file path).
        summary: String,
    },
    /// The result returned to the assistant after a tool call.
    ToolResult {
        /// Whether the tool reported an error.
        is_error: bool,
        /// Content of the tool result.
        content: String,
    },
}

/// Encodes an absolute path into the Claude project path segment used in session file paths.
///
/// Claude Code replaces both `/` and `.` with `-`, so `/Users/me/.hub/x` becomes
/// `-Users-me--hub-x`. Matching this exactly matters for dispatched tasks, whose
/// worktrees live under `~/.hub/workspaces/` (a dotted path); without the dot
/// replacement the session JSONL is never found. Hyphens are preserved.
#[must_use]
pub fn encode_project_path(cwd: &str) -> String {
    cwd.replace(['/', '.'], "-")
}

/// Parses a Claude Code session JSONL file into a sequence of stream blocks.
/// Malformed lines and unknown top-level types are silently skipped.
#[must_use]
pub fn parse_session_jsonl(text: &str) -> Vec<StreamBlock> {
    let mut blocks = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match obj.get("type").and_then(|t| t.as_str()) {
            Some("assistant") => parse_assistant_message(&obj, &mut blocks),
            Some("user") => parse_user_message(&obj, &mut blocks),
            _ => {}
        }
    }
    blocks
}

fn parse_assistant_message(obj: &serde_json::Value, blocks: &mut Vec<StreamBlock>) {
    let Some(arr) = obj
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    else {
        return;
    };
    for block in arr {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("thinking") => {
                let text = block.get("thinking").and_then(|t| t.as_str()).unwrap_or("");
                if !text.is_empty() {
                    blocks.push(StreamBlock::AssistantThinking(text.to_string()));
                }
            }
            Some("text") => {
                let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                if !text.is_empty() {
                    blocks.push(StreamBlock::AssistantText(text.to_string()));
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let input = block
                    .get("input")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let summary = format_tool_input(&name, &input);
                blocks.push(StreamBlock::ToolCall { name, summary });
            }
            _ => {}
        }
    }
}

fn parse_user_message(obj: &serde_json::Value, blocks: &mut Vec<StreamBlock>) {
    let Some(content) = obj.get("message").and_then(|m| m.get("content")) else {
        return;
    };
    if let Some(text) = content.as_str() {
        if !text.is_empty() {
            blocks.push(StreamBlock::HumanTurn(text.to_string()));
        }
    } else if let Some(arr) = content.as_array() {
        for block in arr {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                let is_error = block
                    .get("is_error")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let content = extract_tool_result_content(block);
                blocks.push(StreamBlock::ToolResult { is_error, content });
            }
            // text blocks in user messages are system/skill injections — skip
        }
    }
}

fn format_tool_input(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Bash" => input
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
        "Read" | "Write" | "Edit" => input
            .get("file_path")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string(),
        _ => {
            let s = serde_json::to_string(input).unwrap_or_default();
            if s.len() > 120 {
                format!("{}…", &s[..120])
            } else {
                s
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_project_path_replaces_slashes_and_dots() {
        // Must match the directory Claude Code creates for a task worktree under
        // ~/.hub, e.g. ~/.claude/projects/-Users-michael--hub-workspaces-TASK-0010-hub/
        // The dot in ".hub" is replaced, so it becomes "--hub".
        assert_eq!(
            encode_project_path("/Users/michael/.hub/workspaces/TASK-0010/hub"),
            "-Users-michael--hub-workspaces-TASK-0010-hub"
        );
    }

    #[test]
    fn encode_project_path_preserves_hyphens() {
        assert_eq!(
            encode_project_path("/Users/michael/Repos/ooloth/hub-private"),
            "-Users-michael-Repos-ooloth-hub-private"
        );
    }

    // ── encode_project_path ───────────────────────────────────────────────────

    #[test]
    fn encode_project_path_replaces_slashes_with_dashes() {
        assert_eq!(
            encode_project_path("/Users/michael/Repos/ooloth/hub"),
            "-Users-michael-Repos-ooloth-hub"
        );
    }

    #[test]
    fn encode_project_path_empty_string_returns_empty() {
        assert_eq!(encode_project_path(""), "");
    }

    // ── parse_session_jsonl ───────────────────────────────────────────────────

    fn fixture_jsonl() -> &'static str {
        concat!(
            "{\"type\":\"permission-mode\",\"permissionMode\":\"bypassPermissions\",\"sessionId\":\"s\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Fix the auth bug\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"thinking\",\"thinking\":\"Let me look at auth\",\"signature\":\"sig\"},{\"type\":\"text\",\"text\":\"I'll read the auth file.\"},{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Read\",\"input\":{\"file_path\":\"/src/auth.rs\"}}]}}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"pub fn auth() {}\"}]}}\n",
            "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t2\",\"name\":\"Bash\",\"input\":{\"command\":\"cargo test\",\"description\":\"run\"}},{\"type\":\"tool_use\",\"id\":\"t3\",\"name\":\"Edit\",\"input\":{\"file_path\":\"/src/auth.rs\",\"old_string\":\"old\",\"new_string\":\"new\"}}]}}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t2\",\"content\":\"5 passed\"},{\"type\":\"tool_result\",\"tool_use_id\":\"t3\",\"is_error\":true,\"content\":\"not found\"}]}}\n",
            "{\"type\":\"system\",\"systemPrompt\":\"...\"}\n",
        )
    }

    #[test]
    fn parse_session_jsonl_produces_correct_block_sequence() {
        let blocks = parse_session_jsonl(fixture_jsonl());
        assert_eq!(blocks.len(), 9);
        assert!(matches!(&blocks[0], StreamBlock::HumanTurn(t) if t == "Fix the auth bug"));
        assert!(
            matches!(&blocks[1], StreamBlock::AssistantThinking(t) if t == "Let me look at auth")
        );
        assert!(matches!(&blocks[2], StreamBlock::AssistantText(t) if t.contains("read the auth")));
        assert!(
            matches!(&blocks[3], StreamBlock::ToolCall { name, summary } if name == "Read" && summary == "/src/auth.rs")
        );
        assert!(
            matches!(&blocks[4], StreamBlock::ToolResult { is_error: false, content } if content == "pub fn auth() {}")
        );
        assert!(
            matches!(&blocks[5], StreamBlock::ToolCall { name, summary } if name == "Bash" && summary == "cargo test")
        );
        assert!(
            matches!(&blocks[6], StreamBlock::ToolCall { name, summary } if name == "Edit" && summary == "/src/auth.rs")
        );
        assert!(
            matches!(&blocks[7], StreamBlock::ToolResult { is_error: false, content } if content == "5 passed")
        );
        assert!(
            matches!(&blocks[8], StreamBlock::ToolResult { is_error: true, content } if content == "not found")
        );
    }

    #[test]
    fn parse_session_jsonl_empty_input_returns_empty_vec() {
        assert_eq!(parse_session_jsonl("").len(), 0);
    }

    #[test]
    fn parse_session_jsonl_ignores_unknown_top_level_types() {
        let jsonl = "{\"type\":\"permission-mode\"}\n{\"type\":\"system\",\"systemPrompt\":\"...\"}\n{\"type\":\"file-history-snapshot\"}\n{\"type\":\"ai-title\"}";
        assert_eq!(parse_session_jsonl(jsonl).len(), 0);
    }

    #[test]
    fn parse_session_jsonl_tool_result_array_content_is_joined() {
        let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":[{\"type\":\"text\",\"text\":\"line one\"},{\"type\":\"text\",\"text\":\"line two\"}]}]}}";
        let blocks = parse_session_jsonl(jsonl);
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], StreamBlock::ToolResult { is_error: false, content } if content == "line one\nline two")
        );
    }

    #[test]
    fn parse_session_jsonl_tool_result_is_error_flag() {
        let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"is_error\":true,\"content\":\"boom\"}]}}";
        let blocks = parse_session_jsonl(jsonl);
        assert!(matches!(
            &blocks[0],
            StreamBlock::ToolResult { is_error: true, .. }
        ));
    }

    #[test]
    fn parse_session_jsonl_skips_malformed_json_lines() {
        let jsonl = "not json\n{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\nalso not json";
        let blocks = parse_session_jsonl(jsonl);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], StreamBlock::HumanTurn(t) if t == "hello"));
    }
}

fn extract_tool_result_content(block: &serde_json::Value) -> String {
    match block.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|item| {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    item.get("text")
                        .and_then(|t| t.as_str())
                        .map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}
