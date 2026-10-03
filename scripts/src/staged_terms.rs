//! The commit-time check: nothing staged contains a term banned from hub.
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{bail, Context, Result};

use crate::banned_terms::{self, Hit, TermList};
use crate::scanned_text::{ScannedText, TextOrigin};

/// What a commit's staged content holds.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StagedTerms {
    Clean,
    /// Built from at least one hit, never zero.
    Found {
        first: Hit,
        rest: Vec<Hit>,
    },
}

/// Where the banned-terms list lives for the repository whose shared git directory is
/// `common_dir`: beside the main checkout, which a worktree's own top level is not.
pub(crate) fn banned_terms_path(common_dir: &Path) -> PathBuf {
    let checkout = common_dir.parent().unwrap_or(common_dir);
    let beside = checkout.parent().unwrap_or(checkout);
    beside.join("hub-private/scripts/blocked-terms.txt")
}

/// Checks what is staged in the repository at `repo`.
///
/// # Errors
/// Returns an error when git cannot report what is staged.
pub(crate) fn check(repo: &Path) -> Result<StagedTerms> {
    let common_dir = git_text(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let TermList::Present(terms) =
        banned_terms::load(&banned_terms_path(Path::new(common_dir.trim())))?
    else {
        return Ok(StagedTerms::Clean);
    };

    let staged = git(
        repo,
        &["diff", "--cached", "--name-only", "-z", "--diff-filter=d"],
    )?;
    let mut texts = Vec::new();
    for path in staged
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = String::from_utf8_lossy(path).into_owned();
        let content = git(repo, &["show", &format!(":{path}")])?;
        texts.push(ScannedText {
            origin: TextOrigin::File(PathBuf::from(path)),
            text: String::from_utf8_lossy(&content).into_owned(),
        });
    }

    let mut hits = terms.find_hits(&texts).into_iter();
    Ok(hits
        .next()
        .map_or(StagedTerms::Clean, |first| StagedTerms::Found {
            first,
            rest: hits.collect(),
        }))
}

/// Runs the check on the repository in the current directory, as the pre-commit hook does.
pub(crate) fn run() -> ExitCode {
    let outcome = std::env::current_dir()
        .context("failed to read the current directory")
        .and_then(|dir| check(&dir));
    match outcome {
        Ok(StagedTerms::Clean) => ExitCode::SUCCESS,
        Ok(StagedTerms::Found { first, rest }) => {
            eprintln!("error: staged files contain a term banned from the public hub repo:");
            for hit in std::iter::once(&first).chain(&rest) {
                eprintln!("  {}: \"{}\"", hit.origin, hit.term);
            }
            eprintln!("  move the content to hub-private or replace the term with a generic word");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("error: could not check staged content for banned terms: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Runs git in `repo` and returns its stdout.
fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed ({}): {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn git_text(repo: &Path, args: &[&str]) -> Result<String> {
    String::from_utf8(git(repo, args)?).context("git printed non-UTF-8")
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::scanned_text::TextOrigin;

    const TERM: &str = "alpha";

    fn git(dir: &Path, args: &[&str]) {
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

    /// A parent directory holding `hub/` (a git repo) and `hub-private/` with the list.
    struct Checkouts {
        parent: TempDir,
    }

    impl Checkouts {
        fn new() -> Self {
            let parent = tempfile::tempdir().unwrap();
            let hub = parent.path().join("hub");
            std::fs::create_dir(&hub).unwrap();
            git(&hub, &["init", "-q"]);
            let list = parent.path().join("hub-private/scripts");
            std::fs::create_dir_all(&list).unwrap();
            std::fs::write(list.join("blocked-terms.txt"), format!("{TERM}\n")).unwrap();
            Self { parent }
        }

        fn hub(&self) -> PathBuf {
            self.parent.path().join("hub")
        }

        fn write(&self, name: &str, contents: &str) {
            std::fs::write(self.hub().join(name), contents).unwrap();
        }

        fn stage(&self, name: &str, contents: &str) {
            self.write(name, contents);
            git(&self.hub(), &["add", name]);
        }
    }

    fn found_paths(outcome: &StagedTerms) -> Vec<PathBuf> {
        match outcome {
            StagedTerms::Found { first, rest } => std::iter::once(first)
                .chain(rest)
                .map(|hit| match &hit.origin {
                    TextOrigin::File(path) => path.clone(),
                    TextOrigin::Command => panic!("a staged hit must name its file"),
                })
                .collect(),
            StagedTerms::Clean => vec![],
        }
    }

    #[test]
    fn a_staged_file_with_a_banned_term_is_found() {
        let repo = Checkouts::new();
        repo.stage("notes.md", "about Alpha");

        let outcome = check(&repo.hub()).unwrap();

        assert_eq!(found_paths(&outcome), vec![PathBuf::from("notes.md")]);
    }

    #[test]
    fn clean_staged_content_passes() {
        let repo = Checkouts::new();
        repo.stage("notes.md", "nothing here");

        assert_eq!(check(&repo.hub()).unwrap(), StagedTerms::Clean);
    }

    #[test]
    fn an_unstaged_edit_does_not_count() {
        let repo = Checkouts::new();
        repo.stage("notes.md", "nothing here");
        repo.write("notes.md", "alpha, but not staged");

        assert_eq!(check(&repo.hub()).unwrap(), StagedTerms::Clean);
    }

    #[test]
    fn staged_content_counts_even_after_the_working_copy_is_cleaned() {
        let repo = Checkouts::new();
        repo.stage("notes.md", "alpha");
        repo.write("notes.md", "cleaned on disk only");

        assert_eq!(
            found_paths(&check(&repo.hub()).unwrap()),
            vec![PathBuf::from("notes.md")]
        );
    }

    #[test]
    fn a_staged_deletion_is_not_scanned() {
        let repo = Checkouts::new();
        repo.stage("old.md", "nothing");
        git(&repo.hub(), &["commit", "-q", "-m", "init"]);
        std::fs::write(repo.hub().join("old.md"), "alpha").unwrap();
        git(&repo.hub(), &["commit", "-q", "-am", "add the term"]);
        git(&repo.hub(), &["rm", "-q", "old.md"]);

        assert_eq!(check(&repo.hub()).unwrap(), StagedTerms::Clean);
    }

    #[test]
    fn a_path_with_a_space_is_read() {
        let repo = Checkouts::new();
        repo.stage("my notes.md", "alpha");

        assert_eq!(
            found_paths(&check(&repo.hub()).unwrap()),
            vec![PathBuf::from("my notes.md")]
        );
    }

    #[test]
    fn a_staged_symlink_is_scanned_as_its_target_path_not_the_file_it_points_at() {
        let repo = Checkouts::new();
        let private = repo.parent.path().join("hub-private/notes.md");
        std::fs::write(&private, "alpha").unwrap();
        std::os::unix::fs::symlink(&private, repo.hub().join("linked.md")).unwrap();
        git(&repo.hub(), &["add", "linked.md"]);
        repo.stage("copied.md", "alpha");

        assert_eq!(
            found_paths(&check(&repo.hub()).unwrap()),
            vec![PathBuf::from("copied.md")]
        );
    }

    #[test]
    fn a_worktree_finds_the_list_beside_the_main_checkout() {
        let repo = Checkouts::new();
        repo.stage("readme.md", "start");
        git(&repo.hub(), &["commit", "-q", "-m", "init"]);
        let worktree = repo.hub().join(".claude/worktrees/wt");
        git(
            &repo.hub(),
            &["worktree", "add", "-q", worktree.to_str().unwrap()],
        );
        std::fs::write(worktree.join("notes.md"), "alpha").unwrap();
        git(&worktree, &["add", "notes.md"]);

        assert_eq!(
            found_paths(&check(&worktree).unwrap()),
            vec![PathBuf::from("notes.md")]
        );
    }

    #[test]
    fn the_list_lives_beside_the_checkout_that_owns_the_git_directory() {
        assert_eq!(
            banned_terms_path(Path::new("/code/hub/.git")),
            PathBuf::from("/code/hub-private/scripts/blocked-terms.txt")
        );
    }

    #[test]
    fn without_a_list_everything_passes() {
        let repo = Checkouts::new();
        std::fs::remove_dir_all(repo.parent.path().join("hub-private")).unwrap();
        repo.stage("notes.md", "alpha");

        assert_eq!(check(&repo.hub()).unwrap(), StagedTerms::Clean);
    }

    #[test]
    fn outside_a_git_repository_the_check_fails() {
        let dir = tempfile::tempdir().unwrap();

        assert!(check(dir.path()).is_err());
    }
}
