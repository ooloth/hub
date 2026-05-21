use std::path::PathBuf;
use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::fetch::repos_dir;

const PROMPT: &str = include_str!("../../prompts/implement-issue.md");

fn transcripts_dir() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".hub").join("transcripts")
}

/// Returns true if a Claude stderr line looks like a diagnostic/status message
/// rather than tool output or file content.
///
/// Claude's own diagnostic messages are short single-line status updates (e.g.
/// "Waiting for API…", model-load notices, process errors). Tool call results
/// and file contents that can leak into stderr are typically long or
/// JSON-encoded. This heuristic errs on the side of silence: a line that
/// might be personal data is suppressed from caller stderr even if it happens
/// to be short. The full stderr is always written to the transcript for local
/// debugging.
fn is_claude_diagnostic(line: &str) -> bool {
    // Suppress anything over 200 chars — diagnostic messages are short.
    if line.len() > 200 {
        return false;
    }
    // Suppress lines that look like JSON (tool results, API responses).
    let trimmed = line.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return false;
    }
    true
}

/// Formats a single stream-json event line into a human-readable transcript
/// fragment. Returns `None` for event types that don't contribute readable
/// content (e.g. rate_limit_event, stream_event deltas, system status pings).
fn format_stream_event(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    match v["type"].as_str()? {
        "system" if v["subtype"].as_str() == Some("init") => {
            let model = v["model"].as_str().unwrap_or("?");
            let session = v["session_id"].as_str().unwrap_or("?");
            Some(format!(
                "## Session\nModel: {model}\nSession: {session}\n\n"
            ))
        }
        "assistant" => {
            let content = v["message"]["content"].as_array()?;
            let mut out = String::new();
            for block in content {
                match block["type"].as_str() {
                    Some("thinking") => {
                        let t = block["thinking"].as_str().unwrap_or("");
                        if !t.is_empty() {
                            out.push_str("<thinking>\n");
                            out.push_str(t);
                            out.push_str("\n</thinking>\n\n");
                        }
                    }
                    Some("text") => {
                        let t = block["text"].as_str().unwrap_or("");
                        if !t.is_empty() {
                            out.push_str(t);
                            out.push_str("\n\n");
                        }
                    }
                    Some("tool_use") => {
                        let name = block["name"].as_str().unwrap_or("?");
                        let input =
                            serde_json::to_string_pretty(&block["input"]).unwrap_or_default();
                        out.push_str(&format!("**Tool:** {name}\n```json\n{input}\n```\n\n"));
                    }
                    _ => {}
                }
            }
            if out.is_empty() {
                None
            } else {
                Some(out)
            }
        }
        "user" => {
            let content = v["message"]["content"].as_array()?;
            let mut out = String::new();
            for block in content {
                if block["type"].as_str() != Some("tool_result") {
                    continue;
                }
                let is_error = block["is_error"].as_bool().unwrap_or(false);
                let label = if is_error {
                    "**Error:**"
                } else {
                    "**Result:**"
                };
                // content can be a string or an array of content blocks
                let text = if let Some(s) = block["content"].as_str() {
                    s.to_owned()
                } else if let Some(arr) = block["content"].as_array() {
                    arr.iter()
                        .filter_map(|b| b["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    continue;
                };
                const MAX: usize = 4096;
                let display = if text.len() > MAX {
                    format!(
                        "{}\n\n[...{} chars truncated]",
                        &text[..MAX],
                        text.len() - MAX
                    )
                } else {
                    text
                };
                out.push_str(&format!("{label}\n```\n{display}\n```\n\n"));
            }
            if out.is_empty() {
                None
            } else {
                Some(out)
            }
        }
        "result" => {
            let duration_s = v["duration_ms"].as_u64().unwrap_or(0) / 1000;
            let cost = v["total_cost_usd"].as_f64().unwrap_or(0.0);
            let turns = v["num_turns"].as_u64().unwrap_or(0);
            let status = if v["is_error"].as_bool().unwrap_or(false) {
                "error"
            } else {
                "success"
            };
            Some(format!(
                "---\n\n## Run complete\nStatus: {status}\nTurns: {turns}\nDuration: \
                 {duration_s}s\nCost: ${cost:.4}\n"
            ))
        }
        _ => None,
    }
}

/// Wires hub-private symlinks into `worktree` by calling setup-private.sh with
/// the worktree as root. Skips silently if hub.toml is not a symlink (no private
/// setup on this machine). Logs a warning on script failure but does not abort.
async fn setup_private_in_worktree(worktree: &std::path::Path) {
    let Ok(link_target) = std::fs::read_link("hub.toml") else {
        return;
    };
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    let abs = cwd.join(link_target);
    let Some(device) = abs.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
        return;
    };
    let Some(hub_private) = abs.parent().and_then(|p| p.parent()) else {
        return;
    };
    let script = worktree.join("scripts/setup-private.sh");
    let out = Command::new("bash")
        .args([
            script.to_string_lossy().as_ref(),
            device.as_str(),
            hub_private.to_string_lossy().as_ref(),
            worktree.to_string_lossy().as_ref(),
        ])
        .output()
        .await;
    if let Ok(out) = out {
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            eprintln!("warning: setup-private.sh failed in worktree: {stderr}");
        }
    }
}

/// Implements a single GitHub issue by setting up a worktree and invoking
/// `claude -p` with the implement-issue prompt. Tears down the worktree on
/// clean exit; leaves it in place on process failure for inspection.
/// The full agent transcript (stdout + stderr) is written to
/// `~/.hub/transcripts/<timestamp>-<name>-<issue>.md`.
pub async fn run_one(name: &str, repo: &str, issue: u64) -> Result<()> {
    let bare = repos_dir().join(name);
    let branch = format!("issue-{issue}");
    let worktree = bare.join(&branch);
    let bare_str = bare.to_string_lossy().into_owned();
    let worktree_str = worktree.to_string_lossy().into_owned();

    let fetch = Command::new("git")
        .args(["-C", &bare_str, "fetch", "origin"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await
        .with_context(|| format!("git fetch failed for {name}"))?;
    if !fetch.status.success() {
        let stderr = String::from_utf8_lossy(&fetch.stderr);
        anyhow::bail!("git fetch failed for {name}: {stderr}");
    }

    let add = Command::new("git")
        .args([
            "-C",
            &bare_str,
            "worktree",
            "add",
            &worktree_str,
            "-b",
            &branch,
            "origin/HEAD",
        ])
        .output()
        .await
        .with_context(|| format!("git worktree add failed for {branch}"))?;
    if !add.status.success() {
        let stderr = String::from_utf8_lossy(&add.stderr);
        anyhow::bail!("git worktree add failed for {branch}: {stderr}");
    }

    let cred = Command::new("git")
        .args([
            "-C",
            &worktree_str,
            "config",
            "credential.helper",
            "!gh auth git-credential",
        ])
        .output()
        .await
        .with_context(|| format!("git config credential.helper failed for {branch}"))?;
    if !cred.status.success() {
        let stderr = String::from_utf8_lossy(&cred.stderr);
        anyhow::bail!("git config credential.helper failed for {branch}: {stderr}");
    }

    setup_private_in_worktree(&worktree).await;

    let task = format!(
        "Implement GitHub issue #{issue} in repo {repo}. Worktree: {worktree_str}. Branch: {branch}."
    );

    let ts = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S");
    let transcript_dir = transcripts_dir();
    tokio::fs::create_dir_all(&transcript_dir)
        .await
        .context("failed to create transcripts dir")?;
    let transcript_path = transcript_dir.join(format!("{ts}-{name}-{issue}.md"));
    let mut transcript = tokio::fs::File::create(&transcript_path)
        .await
        .context("failed to create transcript file")?;

    let header =
        format!("# hub implement: {repo}#{issue}\nBranch: {branch}\nStarted: {ts}\n\n---\n\n");
    transcript.write_all(header.as_bytes()).await?;

    eprintln!("hub implement: starting {repo}#{issue}");

    let mut child = Command::new("claude")
        .args([
            "-p",
            "--dangerously-skip-permissions",
            "--verbose",
            "--output-format",
            "stream-json",
            "--allowedTools",
            "Bash,Read,Edit,Write,Skill",
            "--model",
            "opus",
            "--system-prompt",
            PROMPT,
            &task,
        ])
        .current_dir(&worktree)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn claude")?;

    let mut stdout = BufReader::new(child.stdout.take().expect("stdout piped")).lines();
    let mut stderr = BufReader::new(child.stderr.take().expect("stderr piped")).lines();

    let mut stdout_done = false;
    let mut stderr_done = false;

    while !stdout_done || !stderr_done {
        tokio::select! {
            line = stdout.next_line(), if !stdout_done => {
                match line.context("error reading claude stdout")? {
                    Some(l) => {
                        if let Some(formatted) = format_stream_event(&l) {
                            print!("{formatted}");
                            transcript.write_all(formatted.as_bytes()).await?;
                        }
                    }
                    None => stdout_done = true,
                }
            }
            line = stderr.next_line(), if !stderr_done => {
                match line.context("error reading claude stderr")? {
                    Some(l) => {
                        // Always write to transcript for local debugging.
                        transcript.write_all(l.as_bytes()).await?;
                        transcript.write_all(b"\n").await?;
                        // Only forward lines that look like Claude's own
                        // diagnostic output — not tool results or file
                        // contents that may contain personal data.
                        if is_claude_diagnostic(&l) {
                            eprintln!("{l}");
                        }
                    }
                    None => stderr_done = true,
                }
            }
        }
    }

    transcript
        .flush()
        .await
        .context("failed to flush transcript")?;

    let status = child.wait().await.context("claude exited abnormally")?;

    let rm = Command::new("git")
        .args([
            "-C",
            &bare_str,
            "worktree",
            "remove",
            "--force",
            &worktree_str,
        ])
        .output()
        .await
        .with_context(|| format!("git worktree remove failed for {branch}"))?;
    if !rm.status.success() {
        let stderr = String::from_utf8_lossy(&rm.stderr);
        eprintln!("warning: git worktree remove failed for {branch}: {stderr}");
    }

    eprintln!(
        "hub implement: transcript saved to {}",
        transcript_path.display()
    );

    if !status.success() {
        anyhow::bail!("claude exited non-zero for {repo}#{issue}");
    }

    eprintln!("hub implement: done {repo}#{issue}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_claude_diagnostic;

    #[test]
    fn short_status_messages_are_diagnostic() {
        assert!(is_claude_diagnostic("Waiting for API…"));
        assert!(is_claude_diagnostic("hub implement: starting ooloth/hub#81"));
        assert!(is_claude_diagnostic("Error: something went wrong"));
        assert!(is_claude_diagnostic(""));
    }

    #[test]
    fn json_lines_are_not_diagnostic() {
        assert!(!is_claude_diagnostic(
            r#"{"type":"tool_result","content":"hello@example.com"}"#
        ));
        assert!(!is_claude_diagnostic(r#"[{"key":"value"}]"#));
    }

    #[test]
    fn long_lines_are_not_diagnostic() {
        let long = "x".repeat(201);
        assert!(!is_claude_diagnostic(&long));
    }

    #[test]
    fn lines_at_length_boundary_are_handled() {
        let exactly_200 = "x".repeat(200);
        assert!(is_claude_diagnostic(&exactly_200));
        let over_200 = "x".repeat(201);
        assert!(!is_claude_diagnostic(&over_200));
    }
}
