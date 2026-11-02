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

/// The one workspace member that carries its own lints: the crate holding hub's only `unsafe`
/// code. See `docs/invariants/unsafe-code-lives-only-in-disclaimed-spawn.md`.
const UNSAFE_CODE_MEMBER: &str = "disclaimed-spawn";

/// The `[lints]` table [`UNSAFE_CODE_MEMBER`] must carry: the workspace's, with `unsafe_code`
/// relaxed from "forbid" to "deny" so each `unsafe` block can be allowed where it is written.
fn lints_with_unsafe_code_denied(workspace_lints: &toml::Table) -> toml::Table {
    let mut lints = workspace_lints.clone();
    if let Some(toml::Value::Table(rust)) = lints.get_mut("rust") {
        let _ = rust.insert("unsafe_code".into(), toml::Value::String("deny".into()));
    }
    lints
}

/// The `[lints]` table in a manifest's text, or `None` when it has none.
///
/// # Errors
/// Returns an error when the text is not valid TOML or `lints` is not a table.
fn lints_table(manifest: &str) -> Result<Option<toml::Table>> {
    let manifest: toml::Table = manifest.parse().context("manifest is not valid TOML")?;
    manifest
        .get("lints")
        .map(|lints| lints.as_table().cloned().context("`lints` is not a table"))
        .transpose()
}

/// The `[workspace.lints]` table in the root manifest's text.
///
/// # Errors
/// Returns an error when the text is not valid TOML or has no `[workspace.lints]` table.
fn workspace_lints(root_manifest: &str) -> Result<toml::Table> {
    let manifest: toml::Table = root_manifest
        .parse()
        .context("root manifest is not valid TOML")?;
    manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("lints"))
        .and_then(toml::Value::as_table)
        .cloned()
        .context("root manifest has no [workspace.lints] table")
}

/// A crate without `[lints] workspace = true` is compiled without the workspace's clippy and
/// rustc settings, so it passes `just check` while holding none of the standards the rest of the
/// tree is held to. Nothing else reports that, because a missing opt-in looks exactly like a
/// clean crate. The one exception, [`UNSAFE_CODE_MEMBER`], is held to an exact copy of the
/// workspace's lints instead, so it differs from every other crate only in being able to allow
/// `unsafe` where it is written.
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
    assert!(
        members
            .iter()
            .any(|member| member.name == UNSAFE_CODE_MEMBER),
        "cargo metadata did not list {UNSAFE_CODE_MEMBER}, so its lints went unchecked"
    );
    let root = crate::repo_root::repo_root().join("Cargo.toml");
    let workspace = workspace_lints(&std::fs::read_to_string(&root).unwrap()).unwrap();
    let expected_own = lints_with_unsafe_code_denied(&workspace);

    let wrong: Vec<String> = members
        .iter()
        .filter_map(|member| {
            if member.name == UNSAFE_CODE_MEMBER {
                let text = std::fs::read_to_string(&member.manifest).unwrap();
                let own = lints_table(&text).unwrap();
                return (own.as_ref() != Some(&expected_own)).then(|| {
                    format!(
                        "  {} must carry the workspace's lints with unsafe_code = \"deny\", and nothing else",
                        member.manifest.display()
                    )
                });
            }
            match LintOptIn::read(&member.manifest).unwrap() {
                LintOptIn::Inherited => None,
                other => Some(format!("  {} ({other:?})", member.manifest.display())),
            }
        })
        .collect();

    assert!(
        wrong.is_empty(),
        "these manifests need\n\n[lints]\n  workspace = true\n\nto be held to the workspace lints:\n{}",
        wrong.join("\n")
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

    fn table(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    #[test]
    fn the_unsafe_code_crate_carries_the_workspace_lints_with_unsafe_code_denied() {
        let workspace = table(
            "[rust]\nunsafe_code = \"forbid\"\nunused_results = \"deny\"\n\n[clippy]\npedantic = { level = \"warn\", priority = -1 }\n",
        );

        let own = lints_with_unsafe_code_denied(&workspace);

        assert_eq!(
            own,
            table("[rust]\nunsafe_code = \"deny\"\nunused_results = \"deny\"\n\n[clippy]\npedantic = { level = \"warn\", priority = -1 }\n")
        );
    }

    #[rstest]
    #[case::own_table(
        "[package]\nname = \"x\"\n\n[lints.rust]\nunsafe_code = \"deny\"\n",
        Some("[rust]\nunsafe_code = \"deny\"\n")
    )]
    #[case::no_table("[package]\nname = \"x\"\n", None)]
    fn a_manifests_lints_table_is_read_as_written(
        #[case] manifest: &str,
        #[case] expected: Option<&str>,
    ) {
        assert_eq!(lints_table(manifest).unwrap(), expected.map(table));
    }

    #[test]
    fn the_workspace_lints_are_read_from_the_root_manifest() {
        let root =
            "[workspace]\nmembers = []\n\n[workspace.lints.rust]\nunsafe_code = \"forbid\"\n";

        assert_eq!(
            workspace_lints(root).unwrap(),
            table("[rust]\nunsafe_code = \"forbid\"\n")
        );
    }

    #[test]
    fn a_root_manifest_without_workspace_lints_is_an_error() {
        assert!(workspace_lints("[workspace]\nmembers = []\n").is_err());
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
