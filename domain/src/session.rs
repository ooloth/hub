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
    HumanTurn(String),
    AssistantText(String),
    AssistantThinking(String),
    ToolCall { name: String, summary: String },
    ToolResult { is_error: bool, content: String },
}

/// Encodes an absolute path into the Claude project path segment used in session file paths.
/// Replaces every `/` with `-`, including the leading one.
pub fn encode_project_path(cwd: &str) -> String {
    cwd.replace('/', "-")
}

/// Parses a Claude Code session JSONL file into a sequence of stream blocks.
/// Malformed lines and unknown top-level types are silently skipped.
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
                    .and_then(|e| e.as_bool())
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
