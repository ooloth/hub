use std::path::Path;

use anyhow::{Context, Result};

use super::workspace::WorkspaceMember;

/// Whether a crate's manifest opts into the workspace's lints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LintOptIn {
    /// `[lints]` with `workspace = true`.
    Inherited,
    /// No `[lints]` table at all.
    NoLintsTable,
    /// A `[lints]` table that does not set `workspace = true`.
    NotInherited,
}

impl LintOptIn {
    /// Reads the opt-in from a manifest's text.
    ///
    /// # Errors
    /// Returns an error when the text is not valid TOML.
    fn parse(manifest: &str) -> Result<Self> {
        let manifest: toml::Table = manifest.parse().context("manifest is not valid TOML")?;
        let Some(lints) = manifest.get("lints") else {
            return Ok(Self::NoLintsTable);
        };
        let inherited = lints.get("workspace").and_then(toml::Value::as_bool) == Some(true);
        Ok(if inherited {
            Self::Inherited
        } else {
            Self::NotInherited
        })
    }

    /// Reads the opt-in from the manifest at `path`.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read or is not valid TOML.
    fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }
}

/// A crate without `[lints] workspace = true` is compiled without the workspace's clippy and
/// rustc settings, so it passes `just check` while holding none of the standards the rest of the
/// tree is held to. Nothing else reports that, because a missing opt-in looks exactly like a
/// clean crate.
#[test]
fn every_workspace_member_inherits_the_workspace_lints() {
    let members = WorkspaceMember::of_this_repo().unwrap();
    assert!(
        !members.is_empty(),
        "cargo metadata listed no workspace members"
    );
    assert!(
        members.iter().any(|member| member.name == "scripts"),
        "cargo metadata did not list this crate, so the member list cannot be trusted"
    );

    let missing: Vec<String> = members
        .iter()
        .filter_map(|member| match LintOptIn::read(&member.manifest).unwrap() {
            LintOptIn::Inherited => None,
            other => Some(format!("  {} ({other:?})", member.manifest.display())),
        })
        .collect();

    assert!(
        missing.is_empty(),
        "these manifests need\n\n[lints]\n  workspace = true\n\nto be held to the workspace lints:\n{}",
        missing.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::inherited(
        "[package]\nname = \"x\"\n\n[lints]\n  workspace = true\n",
        LintOptIn::Inherited
    )]
    #[case::no_table("[package]\nname = \"x\"\n", LintOptIn::NoLintsTable)]
    #[case::opted_out("[lints]\nworkspace = false\n", LintOptIn::NotInherited)]
    #[case::own_lints_only("[lints.clippy]\npedantic = \"warn\"\n", LintOptIn::NotInherited)]
    #[case::dependency_inheritance_is_not_lint_inheritance(
        "[package]\nname = \"x\"\n\n[dependencies]\nanyhow = { workspace = true }\n",
        LintOptIn::NoLintsTable
    )]
    fn the_opt_in_is_read_from_the_lints_table(
        #[case] manifest: &str,
        #[case] expected: LintOptIn,
    ) {
        assert_eq!(LintOptIn::parse(manifest).unwrap(), expected);
    }

    #[test]
    fn a_manifest_that_is_not_toml_is_an_error() {
        assert!(LintOptIn::parse("[lints\nworkspace = true").is_err());
    }

    #[test]
    fn an_unreadable_manifest_is_an_error() {
        let dir = tempfile::tempdir().unwrap();

        assert!(LintOptIn::read(&dir.path().join("Cargo.toml")).is_err());
    }
}
