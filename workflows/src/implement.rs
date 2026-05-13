use anyhow::{Context, Result};
use tokio::process::Command;

use crate::fetch::repos_dir;

const PROMPT: &str = include_str!("../../prompts/implement-issue.md");

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

    let task = format!(
        "Implement GitHub issue #{issue} in repo {repo}. Worktree: {worktree_str}. Branch: {branch}."
    );
    eprintln!("hub implement: starting {repo}#{issue}");

    let status = Command::new("claude")
        .args([
            "-p",
            "--dangerously-skip-permissions",
            "--allowedTools",
            "Bash,Read,Edit,Write,Skill",
            "--model",
            "opus",
            "--system-prompt",
            PROMPT,
            &task,
        ])
        .status()
        .await
        .context("failed to spawn claude")?;

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
