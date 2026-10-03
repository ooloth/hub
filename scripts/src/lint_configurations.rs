//! Clippy in every configuration this checkout can build.
//! See docs/invariants/hub-builds-with-and-without-each-private-module.md
use std::fmt;
use std::path::Path;
use std::process::{Command, ExitCode};

use anyhow::{Context, Result};

use crate::repo_root::repo_root;

/// One way this checkout can be built, and the cargo arguments that build it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Configuration {
    pub(crate) name: &'static str,
    pub(crate) cargo_args: &'static [&'static str],
}

const WITHOUT_PRIVATE: Configuration = Configuration {
    name: "without hub-private",
    cargo_args: &[],
};
const PRIVATE: Configuration = Configuration {
    name: "hub-private without media",
    cargo_args: &["--features", "private"],
};
const MEDIA: Configuration = Configuration {
    name: "hub-private with media",
    cargo_args: &["-p", "hub-tui", "--features", "media"],
};

impl Configuration {
    /// Every configuration whose sources are present under `root`, in a fixed order.
    pub(crate) fn present_in(root: &Path) -> Vec<Self> {
        let mut present = vec![WITHOUT_PRIVATE];
        if root.join("clients/src/private").exists() {
            present.push(PRIVATE);
        }
        if root.join("ui/tui/src/investigations/media.rs").exists() {
            present.push(MEDIA);
        }
        present
    }
}

/// Which configurations failed to lint.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct LintSummary {
    pub(crate) failed: Vec<&'static str>,
}

impl fmt::Display for LintSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.failed.is_empty() {
            return Ok(());
        }
        write!(
            f,
            "clippy failed in: {}\n\n\
             Every change must build on devices with and without each hub-private module,\n\
             not only on the device it was written on. See\n\
             docs/invariants/hub-builds-with-and-without-each-private-module.md",
            self.failed.join(", ")
        )
    }
}

/// Runs clippy with `-D warnings` in `configuration`, letting its output through.
///
/// # Errors
/// Returns an error when cargo cannot be started.
fn lint(root: &Path, configuration: Configuration) -> Result<bool> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    eprintln!("clippy: {}", configuration.name);
    let status = Command::new(&cargo)
        .arg("clippy")
        .args(configuration.cargo_args)
        .args(["--", "-D", "warnings"])
        .current_dir(root)
        .status()
        .with_context(|| format!("failed to run {} clippy", cargo.to_string_lossy()))?;
    Ok(status.success())
}

/// Lints every configuration present in this checkout, then names any that failed.
pub(crate) fn run() -> ExitCode {
    let root = repo_root();
    let mut summary = LintSummary::default();
    for configuration in Configuration::present_in(&root) {
        match lint(&root, configuration) {
            Ok(true) => {}
            Ok(false) => summary.failed.push(configuration.name),
            Err(error) => {
                eprintln!("error: {error:#}");
                summary.failed.push(configuration.name);
            }
        }
    }
    if summary.failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        eprintln!("\n{summary}");
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn checkout_with(paths: &[&str]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for path in paths {
            let path = root.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        root
    }

    #[rstest]
    #[case::public_only(&[], &["without hub-private"])]
    #[case::private(&["clients/src/private/mod.rs"], &["without hub-private", "hub-private without media"])]
    #[case::media_only(&["ui/tui/src/investigations/media.rs"], &["without hub-private", "hub-private with media"])]
    #[case::both(
        &["clients/src/private/mod.rs", "ui/tui/src/investigations/media.rs"],
        &["without hub-private", "hub-private without media", "hub-private with media"]
    )]
    fn the_configurations_are_those_whose_sources_are_present(
        #[case] paths: &[&str],
        #[case] expected: &[&str],
    ) {
        let root = checkout_with(paths);

        let names: Vec<&str> = Configuration::present_in(root.path())
            .iter()
            .map(|configuration| configuration.name)
            .collect();

        assert_eq!(names, expected);
    }

    #[test]
    fn each_configuration_builds_with_the_arguments_the_invariant_names() {
        let root = checkout_with(&[
            "clients/src/private/mod.rs",
            "ui/tui/src/investigations/media.rs",
        ]);

        let args: Vec<&[&str]> = Configuration::present_in(root.path())
            .iter()
            .map(|configuration| configuration.cargo_args)
            .collect();

        assert_eq!(
            args,
            vec![
                &[][..],
                &["--features", "private"][..],
                &["-p", "hub-tui", "--features", "media"][..],
            ]
        );
    }

    #[test]
    fn a_summary_names_every_failed_configuration_and_the_invariant() {
        let summary = LintSummary {
            failed: vec!["without hub-private", "hub-private with media"],
        };

        let message = summary.to_string();

        assert!(message.contains("without hub-private"), "{message}");
        assert!(message.contains("hub-private with media"), "{message}");
        assert!(
            message.contains("hub-builds-with-and-without-each-private-module.md"),
            "{message}"
        );
    }

    #[test]
    fn a_clean_summary_says_nothing() {
        assert_eq!(LintSummary::default().to_string(), "");
    }
}
