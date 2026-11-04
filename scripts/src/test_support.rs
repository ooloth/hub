//! Helpers shared by the scripts crate's tests.
use std::path::Path;
use std::process::Command;

/// Runs git in `dir` with a fixed identity and `main` as the default branch, and fails the test
/// when git does.
pub(crate) fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}
