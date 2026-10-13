use anyhow::{Context, Result};

use crate::repo_root::repo_root;

/// What the dev profile must set, and the test profile must leave to it.
///
/// On macOS, debug information left unpacked stays in the object files, so rustc keeps every
/// rebuild's object files beside the binaries and never removes the old ones: about 2,600 more per
/// rebuild. They reached 697,000 files in `target/debug/deps`, and every test that builds an HTTP
/// client lists that directory, which took tests from 3 seconds to 40. Packed, the debug
/// information goes into a `.dSYM` bundle each rebuild replaces. Line tables only keep backtraces
/// pointing at file and line while packing in about half a second.
const REQUIRED: [(&str, &str); 2] = [("split-debuginfo", "packed"), ("debug", "line-tables-only")];

/// Every way the workspace manifest's text falls short of [`REQUIRED`], one line each.
///
/// # Errors
/// Returns an error when the text is not valid TOML.
fn shortfalls(manifest: &str) -> Result<Vec<String>> {
    let manifest: toml::Table = manifest.parse().context("manifest is not valid TOML")?;
    let profile = |name: &str| {
        manifest
            .get("profile")
            .and_then(|profiles| profiles.get(name))
    };
    let dev = profile("dev");
    let test = profile("test");
    let mut shortfalls = Vec::new();
    for (key, value) in REQUIRED {
        let set = dev
            .and_then(|dev| dev.get(key))
            .and_then(toml::Value::as_str);
        if set != Some(value) {
            shortfalls.push(format!("  [profile.dev] {key} is {set:?}, not \"{value}\""));
        }
        if test.and_then(|test| test.get(key)).is_some() {
            shortfalls.push(format!(
                "  [profile.test] sets {key}, which it must leave to dev"
            ));
        }
    }
    Ok(shortfalls)
}

/// Without this, an edit to the dev profile brings the object files back, and nothing says so
/// until tests take minutes again, which is how it went unnoticed the first time.
#[test]
fn the_dev_profile_packs_line_tables_and_the_test_profile_inherits_them() {
    let path = repo_root().join("Cargo.toml");
    let manifest = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))
        .unwrap();

    let shortfalls = shortfalls(&manifest).unwrap();

    assert!(
        shortfalls.is_empty(),
        "{} needs\n\n[profile.dev]\n  debug = \"line-tables-only\"\n  split-debuginfo = \"packed\"\n\n\
         and no override of either under [profile.test]:\n{}",
        path.display(),
        shortfalls.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const PACKED: &str =
        "[profile.dev]\n  debug = \"line-tables-only\"\n  split-debuginfo = \"packed\"\n";

    #[test]
    fn a_dev_profile_that_packs_line_tables_falls_short_of_nothing() {
        assert_eq!(shortfalls(PACKED).unwrap(), Vec::<String>::new());
    }

    #[rstest]
    #[case::no_dev_profile("[workspace]\nmembers = []\n", 2)]
    #[case::unpacked(
        "[profile.dev]\n  debug = \"line-tables-only\"\n  split-debuginfo = \"unpacked\"\n",
        1
    )]
    #[case::full_debug_info("[profile.dev]\n  debug = true\n  split-debuginfo = \"packed\"\n", 1)]
    #[case::test_profile_unpacks(
        "[profile.dev]\n  debug = \"line-tables-only\"\n  split-debuginfo = \"packed\"\n\n\
         [profile.test]\n  split-debuginfo = \"unpacked\"\n",
        1
    )]
    #[case::test_profile_sets_debug(
        "[profile.dev]\n  debug = \"line-tables-only\"\n  split-debuginfo = \"packed\"\n\n\
         [profile.test]\n  debug = \"line-tables-only\"\n",
        1
    )]
    fn each_way_the_profiles_fall_short_is_named(#[case] manifest: &str, #[case] count: usize) {
        assert_eq!(shortfalls(manifest).unwrap().len(), count, "{manifest}");
    }

    #[test]
    fn a_test_profile_that_sets_neither_key_is_fine() {
        let manifest = format!("{PACKED}\n[profile.test]\n  opt-level = 1\n");

        assert_eq!(shortfalls(&manifest).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_manifest_that_is_not_toml_is_an_error() {
        assert!(shortfalls("[profile.dev\ndebug = 0").is_err());
    }
}
