//! Starting `op` as an ordinary child or as one that answers for itself, and reading what it
//! returns. The one place the two spawns part.
use anyhow::{Context, Result};
use domain::op_spawn::OpSpawn;

/// What `op read` returns for `reference`, from an `op` started as `spawn`.
pub(crate) async fn read_reference(reference: &str, spawn: OpSpawn) -> Result<String> {
    let output = start_op("op", &["read", reference], spawn)
        .await
        .map_err(|error| {
            let why = why_op_did_not_start(&error);
            anyhow::Error::new(error).context(why)
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "failed to resolve credential from 1Password: {}",
            stderr.trim()
        );
    }
    let resolved = String::from_utf8(output.stdout).context("op read returned non-UTF-8 output")?;
    Ok(resolved.trim().to_string())
}

/// Runs `program` with `args`, started as `spawn`, and returns everything it wrote and its
/// status. This is the one place the two spawns part.
async fn start_op(
    program: &str,
    args: &[&str],
    spawn: OpSpawn,
) -> std::io::Result<std::process::Output> {
    match spawn {
        OpSpawn::Inherited => killed_on_drop(program).args(args).output().await,
        OpSpawn::Disclaimed => start_disclaimed(program, args).await,
    }
}

/// What starting `op` failed on, in words that point at the fix. `op`'s own error follows it.
fn why_op_did_not_start(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => "`op` is not on the PATH; is the 1Password CLI installed?",
        // Only the disclaimed spawn reports this, when macOS lacks the function that disclaims.
        std::io::ErrorKind::Unsupported => "macOS cannot start `op` so that it answers for itself",
        _ => "failed to start `op read`",
    }
}

/// Runs `program` so that it answers for itself, and returns everything it wrote and its status.
///
/// On macOS that means disclaiming responsibility, because macOS otherwise judges a child by the
/// process that started it (decision 027's addendum of 2026-11-01). Linux holds no process
/// responsible for another, so there an ordinary child already answers for itself (decision 031).
async fn start_disclaimed(program: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    #[cfg(target_os = "macos")]
    let output = disclaimed_spawn::spawn(program, args)?.output().await;
    #[cfg(not(target_os = "macos"))]
    let output = killed_on_drop(program).args(args).output().await;
    output
}

/// A command whose process is killed when the future running it is dropped.
///
/// A read gives up on `op` at its deadline by dropping its future. Without this, an `op` that never
/// answers would keep running after the attempt that gave up on it. `screen_look` starts
/// `osascript` and `ps` through it too, under deadlines of their own.
pub(crate) fn killed_on_drop(program: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    let _ = command.kill_on_drop(true);
    command
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn a_process_still_running_at_the_deadline_is_gone_straight_after_it() {
        let read = killed_on_drop("sleep").arg("30").output();

        let timed = tokio::time::timeout(Duration::from_millis(200), read).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(timed.is_err(), "sleep 30 finished within 200 ms");
        // Options before the pattern: BSD pgrep stops reading options at the pattern, so a `-P`
        // after it is taken as another pattern, and any `sleep` on the machine would match.
        let children = std::process::Command::new("pgrep")
            .args(["-P", &std::process::id().to_string(), "-x", "sleep"])
            .output()
            .unwrap();
        assert!(
            children.stdout.is_empty(),
            "a sleep child is still running: {}",
            String::from_utf8_lossy(&children.stdout)
        );
    }

    #[rstest::rstest]
    #[case::not_found(
        std::io::ErrorKind::NotFound,
        "`op` is not on the PATH; is the 1Password CLI installed?"
    )]
    #[case::unsupported(
        std::io::ErrorKind::Unsupported,
        "macOS cannot start `op` so that it answers for itself"
    )]
    #[case::anything_else(std::io::ErrorKind::PermissionDenied, "failed to start `op read`")]
    fn a_failed_start_is_named_by_its_kind(#[case] kind: std::io::ErrorKind, #[case] name: &str) {
        assert_eq!(why_op_did_not_start(&std::io::Error::from(kind)), name);
    }

    /// The pid of a shell started as `spawn`, and the process macOS holds
    /// responsible for a `sleep` that shell started, asked while the shell is still running:
    /// once a disclaimed shell exits, macOS holds the `sleep` responsible for itself instead.
    #[cfg(target_os = "macos")]
    async fn who_a_child_of_the_started_program_answers_to(spawn: OpSpawn) -> (u32, u32) {
        let pids_file =
            std::env::temp_dir().join(format!("hub-config-spawn-{}-{spawn:?}", std::process::id()));
        let _ = std::fs::remove_file(&pids_file);
        let script = format!(
            "sleep 5 >/dev/null 2>&1 & echo $$ $! > '{}'; sleep 2",
            pids_file.display()
        );
        let shell = tokio::spawn(async move { start_op("/bin/sh", &["-c", &script], spawn).await });

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let text = loop {
            match std::fs::read_to_string(&pids_file) {
                Ok(text) if text.ends_with('\n') => break text,
                _ => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "the shell wrote no pids"
                    );
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
        };
        let pids: Vec<u32> = text
            .split_whitespace()
            .map(|pid| pid.parse().unwrap())
            .collect();
        let [shell_pid, child] = pids[..] else {
            panic!("expected two pids, got {text:?}");
        };
        let answers_to = disclaimed_spawn::responsible_for(child);
        let _ = std::process::Command::new("kill")
            .arg(child.to_string())
            .status();
        let _ = shell.await.unwrap().unwrap();
        let _ = std::fs::remove_file(&pids_file);
        (shell_pid, answers_to.unwrap())
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_child_of_a_program_started_disclaimed_answers_to_that_program() {
        let test_answers_to = disclaimed_spawn::responsible_for(std::process::id()).unwrap();

        let (shell, answers_to) =
            who_a_child_of_the_started_program_answers_to(OpSpawn::Disclaimed).await;

        assert_eq!(answers_to, shell);
        assert_ne!(answers_to, test_answers_to);
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_child_of_a_program_started_inherited_answers_to_what_the_test_answers_to() {
        let test_answers_to = disclaimed_spawn::responsible_for(std::process::id()).unwrap();

        let (_, answers_to) =
            who_a_child_of_the_started_program_answers_to(OpSpawn::Inherited).await;

        assert_eq!(answers_to, test_answers_to);
    }

    #[tokio::test]
    async fn a_disclaimed_start_returns_the_programs_exit_status_and_output() {
        let output = start_disclaimed("/bin/sh", &["-c", "printf out; printf err >&2; exit 3"])
            .await
            .unwrap();

        assert_eq!(output.status.code(), Some(3));
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, b"err");
    }
}
