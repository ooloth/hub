use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// A crate in hub's workspace, as `cargo metadata` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceMember {
    pub(crate) name: String,
    pub(crate) manifest: PathBuf,
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

/// The root of hub's repository: the directory above this crate.
pub(crate) fn repo_root() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_dir.parent().unwrap_or(crate_dir).to_path_buf()
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
                },
                WorkspaceMember {
                    name: "hub-tui".to_string(),
                    manifest: PathBuf::from("/repo/ui/tui/Cargo.toml"),
                },
            ]
        );
    }

    #[test]
    fn a_member_lives_in_its_manifests_directory() {
        let member = WorkspaceMember {
            name: "hub-tui".to_string(),
            manifest: PathBuf::from("/repo/ui/tui/Cargo.toml"),
        };

        assert_eq!(member.dir(), Path::new("/repo/ui/tui"));
    }

    #[test]
    fn output_that_is_not_cargo_metadata_is_an_error() {
        assert!(WorkspaceMember::all_from(r#"{"nope":1}"#).is_err());
    }
}
