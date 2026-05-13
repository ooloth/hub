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

fn parse_issue_numbers(output: &str) -> Result<Vec<u64>> {
    output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            l.trim()
                .parse::<u64>()
                .with_context(|| format!("invalid issue number: {l}"))
        })
        .collect()
}

/// Returns all issue numbers labeled `status:ready-for-agent` in `repo`.
async fn ready_issues(repo: &str) -> Result<Vec<u64>> {
    let out = Command::new("gh")
        .args([
            "issue",
            "list",
            "--repo",
            repo,
            "--label",
            "status:ready-for-agent",
            "--json",
            "number",
            "--jq",
            ".[].number",
        ])
        .output()
        .await
        .with_context(|| format!("gh issue list failed for {repo}"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("gh issue list failed for {repo}: {stderr}");
    }

    parse_issue_numbers(&String::from_utf8_lossy(&out.stdout))
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
            "--allowedTools",
            "Bash,Read,Edit,Write,Skill",
            "--model",
            "opus",
            "--system-prompt",
            PROMPT,
            &task,
        ])
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
                        println!("{l}");
                        transcript.write_all(l.as_bytes()).await?;
                        transcript.write_all(b"\n").await?;
                    }
                    None => stdout_done = true,
                }
            }
            line = stderr.next_line(), if !stderr_done => {
                match line.context("error reading claude stderr")? {
                    Some(l) => {
                        eprintln!("{l}");
                        transcript.write_all(l.as_bytes()).await?;
                        transcript.write_all(b"\n").await?;
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

/// Finds all `status:ready-for-agent` issues in opted-in repos and runs
/// `run_one` for each, serially.
pub async fn run_all(repos: &[(String, String)]) -> Result<()> {
    for (name, repo) in repos {
        let issues = ready_issues(repo).await?;
        for issue in issues {
            run_one(name, repo, issue).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_issue_numbers_parses_single() {
        assert_eq!(parse_issue_numbers("42\n").unwrap(), vec![42]);
    }

    #[test]
    fn parse_issue_numbers_parses_multiple() {
        assert_eq!(parse_issue_numbers("1\n2\n3\n").unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn parse_issue_numbers_ignores_blank_lines() {
        assert_eq!(parse_issue_numbers("\n42\n\n7\n").unwrap(), vec![42, 7]);
    }

    #[test]
    fn parse_issue_numbers_returns_empty_for_empty_output() {
        assert!(parse_issue_numbers("").unwrap().is_empty());
    }

    #[test]
    fn parse_issue_numbers_errors_on_non_numeric_line() {
        assert!(parse_issue_numbers("not-a-number\n").is_err());
    }
}
