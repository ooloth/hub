use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::repo_root::repo_root;
use serde::Deserialize;

/// A crate in hub's workspace, as `cargo metadata` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceMember {
    pub(crate) name: String,
    pub(crate) manifest: PathBuf,
    /// The workspace crates it is built from: its normal and build dependencies that cargo
    /// resolves by path. Dev-dependencies are left out, since they never reach a binary.
    pub(crate) local_dependencies: Vec<String>,
}

impl WorkspaceMember {
    /// The directory holding the member's `Cargo.toml`.
    pub(crate) fn dir(&self) -> &Path {
        self.manifest.parent().unwrap_or(&self.manifest)
    }

    /// The members listed in `cargo metadata --no-deps --format-version 1` output.
    ///
    /// # Errors
    /// Returns an error when the JSON is not cargo metadata.
    pub(crate) fn all_from(metadata_json: &str) -> Result<Vec<Self>> {
        let metadata: Metadata =
            serde_json::from_str(metadata_json).context("not cargo metadata output")?;
        Ok(metadata
            .packages
            .into_iter()
            .map(|package| Self {
                name: package.name,
                manifest: package.manifest_path,
                local_dependencies: package
                    .dependencies
                    .into_iter()
                    .filter(|dependency| {
                        dependency.path.is_some() && dependency.kind.as_deref() != Some("dev")
                    })
                    .map(|dependency| dependency.name)
                    .collect(),
            })
            .collect())
    }

    /// The members of this repository's workspace, asking cargo.
    ///
    /// # Errors
    /// Returns an error when cargo cannot be run or its output cannot be read.
    pub(crate) fn of_this_repo() -> Result<Vec<Self>> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let manifest = repo_root().join("Cargo.toml");
        let output = std::process::Command::new(&cargo)
            .args([
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(&manifest)
            .output()
            .with_context(|| format!("failed to run {}", cargo.to_string_lossy()))?;
        if !output.status.success() {
            bail!(
                "cargo metadata failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let json = String::from_utf8(output.stdout).context("cargo metadata printed non-UTF-8")?;
        Self::all_from(&json)
    }
}

/// The part of `cargo metadata` output this module reads.
#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    manifest_path: PathBuf,
    #[serde(default)]
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    /// Present only for a dependency cargo resolves by path, which in this workspace is a member.
    path: Option<PathBuf>,
    /// `None` for a normal dependency, `"dev"` or `"build"` otherwise.
    kind: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn members_are_read_from_cargo_metadata() {
        let json = r#"{"packages":[
            {"name":"store","manifest_path":"/repo/store/Cargo.toml","version":"0.1.0"},
            {"name":"hub-tui","manifest_path":"/repo/ui/tui/Cargo.toml","version":"0.1.0"}
        ],"workspace_root":"/repo"}"#;

        let members = WorkspaceMember::all_from(json).unwrap();

        assert_eq!(
            members,
            vec![
                WorkspaceMember {
                    name: "store".to_string(),
                    manifest: PathBuf::from("/repo/store/Cargo.toml"),
                    local_dependencies: vec![],
                },
                WorkspaceMember {
                    name: "hub-tui".to_string(),
                    manifest: PathBuf::from("/repo/ui/tui/Cargo.toml"),
                    local_dependencies: vec![],
                },
            ]
        );
    }

    #[test]
    fn a_members_local_dependencies_are_its_path_dependencies_that_reach_its_binary() {
        let json = r#"{"packages":[
            {"name":"hub-daemon","manifest_path":"/repo/daemon/Cargo.toml","dependencies":[
                {"name":"store","path":"/repo/store","kind":null},
                {"name":"anyhow","kind":null},
                {"name":"tempfile","kind":"dev"},
                {"name":"store-test-support","path":"/repo/support","kind":"dev"}
            ]}
        ],"workspace_root":"/repo"}"#;

        let members = WorkspaceMember::all_from(json).unwrap();

        assert_eq!(members[0].local_dependencies, ["store"]);
    }

    #[test]
    fn a_member_lives_in_its_manifests_directory() {
        let member = WorkspaceMember {
            name: "hub-tui".to_string(),
            manifest: PathBuf::from("/repo/ui/tui/Cargo.toml"),
            local_dependencies: vec![],
        };

        assert_eq!(member.dir(), Path::new("/repo/ui/tui"));
    }

    #[test]
    fn output_that_is_not_cargo_metadata_is_an_error() {
        assert!(WorkspaceMember::all_from(r#"{"nope":1}"#).is_err());
    }
}
