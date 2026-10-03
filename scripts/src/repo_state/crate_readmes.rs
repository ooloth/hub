use std::fmt;
use std::path::PathBuf;

use super::workspace::WorkspaceMember;

/// What is wrong with a crate's README.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadmeKind {
    Missing,
    /// Present but holding nothing except whitespace.
    Empty,
}

/// A workspace member whose README a contributor could not use.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadmeProblem {
    crate_name: String,
    readme: PathBuf,
    kind: ReadmeKind,
}

impl ReadmeProblem {
    /// The problem with `member`'s README, if it has one.
    fn find(member: &WorkspaceMember) -> Option<Self> {
        let readme = member.dir().join("README.md");
        let kind = match std::fs::read_to_string(&readme) {
            Err(_) => ReadmeKind::Missing,
            Ok(contents) if contents.trim().is_empty() => ReadmeKind::Empty,
            Ok(_) => return None,
        };
        Some(Self {
            crate_name: member.name.clone(),
            readme,
            kind,
        })
    }
}

impl fmt::Display for ReadmeProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            ReadmeKind::Missing => "missing",
            ReadmeKind::Empty => "empty",
        };
        write!(f, "{} ({}): {kind}", self.crate_name, self.readme.display())
    }
}

/// A crate's README is where the next contributor finds how to run it, how to see it
/// working, and which of its surprises will cost them an afternoon. None of that is
/// derivable from the source, and nobody notices it is absent until they need it.
#[test]
fn every_workspace_member_has_a_readme() {
    let members = WorkspaceMember::of_this_repo().unwrap();
    assert!(
        !members.is_empty(),
        "cargo metadata listed no workspace members"
    );
    assert!(
        members.iter().any(|member| member.name == "scripts"),
        "cargo metadata did not list this crate, so the member list cannot be trusted"
    );

    let problems: Vec<String> = members
        .iter()
        .filter_map(ReadmeProblem::find)
        .map(|problem| format!("  {problem}"))
        .collect();

    assert!(
        problems.is_empty(),
        "every workspace member needs a README:\n{}\n\n\
         Cover what the crate is for, how to run it, and the gotchas that are not visible in \
         the source. clients/README.md is the minimal shape; daemon/README.md adds a runbook.",
        problems.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn member_with_readme(readme: Option<&str>) -> (tempfile::TempDir, WorkspaceMember) {
        let dir = tempfile::tempdir().unwrap();
        if let Some(contents) = readme {
            std::fs::write(dir.path().join("README.md"), contents).unwrap();
        }
        let member = WorkspaceMember {
            name: "example".to_string(),
            manifest: dir.path().join("Cargo.toml"),
        };
        (dir, member)
    }

    #[test]
    fn a_crate_with_a_readme_has_no_problem() {
        let (_dir, member) = member_with_readme(Some("# example\n\nWhat it is for.\n"));

        assert_eq!(ReadmeProblem::find(&member), None);
    }

    #[test]
    fn a_crate_without_a_readme_is_missing_one() {
        let (dir, member) = member_with_readme(None);

        assert_eq!(
            ReadmeProblem::find(&member),
            Some(ReadmeProblem {
                crate_name: "example".to_string(),
                readme: dir.path().join("README.md"),
                kind: ReadmeKind::Missing,
            })
        );
    }

    #[rstest]
    #[case::zero_bytes("")]
    #[case::only_whitespace("  \n\t\n")]
    fn a_readme_with_nothing_in_it_is_empty(#[case] contents: &str) {
        let (_dir, member) = member_with_readme(Some(contents));

        let kind = ReadmeProblem::find(&member).map(|problem| problem.kind);

        assert_eq!(kind, Some(ReadmeKind::Empty));
    }

    #[test]
    fn a_problem_names_the_crate_and_what_is_wrong() {
        let (_dir, member) = member_with_readme(None);

        let message = ReadmeProblem::find(&member).unwrap().to_string();

        assert!(message.contains("example"), "{message}");
        assert!(message.contains("missing"), "{message}");
    }
}
