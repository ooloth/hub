use std::path::{Path, PathBuf};

use super::workspace::WorkspaceMember;

/// The name a snapshot file is stored under, which is the name a test asserts it by.
///
/// insta names a file `<crate>__<module path>__<name>.snap`. Pending `.snap.new` files and other
/// files are not snapshots, so they have no name.
fn snapshot_name(file: &Path) -> Option<String> {
    let stem = file.file_name()?.to_str()?.strip_suffix(".snap")?;
    let name = stem.rsplit("__").next()?;
    (!name.is_empty()).then(|| name.to_string())
}

/// Every `.snap` file under any `snapshots` directory in `src`, sorted.
fn snapshot_files(src: &Path) -> Vec<PathBuf> {
    let mut snapshots: Vec<PathBuf> = files_under(src)
        .into_iter()
        .filter(|path| {
            path.parent()
                .and_then(Path::file_name)
                .is_some_and(|dir| dir == "snapshots")
                && path
                    .extension()
                    .is_some_and(|extension| extension == "snap")
        })
        .collect();
    snapshots.sort();
    snapshots
}

/// Every file under `dir`, at any depth. A directory that cannot be read is skipped, since a
/// missing `src` simply has no snapshots.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

/// A snapshot no test produces is dead weight that reads as coverage. This matches each
/// snapshot's name against the crate's source, which works in every configuration, including
/// checkouts without hub-private where feature-gated tests do not compile. Its blind spot: a name
/// mentioned in source without any test producing that snapshot passes.
#[test]
fn every_snapshot_is_produced_by_a_test() {
    let members = WorkspaceMember::of_this_repo().unwrap();
    let mut scanned = 0;
    let mut orphans = Vec::new();

    for member in &members {
        let src = member.dir().join("src");
        let snapshots = snapshot_files(&src);
        if snapshots.is_empty() {
            continue;
        }
        let source = rust_source(&src);
        for snapshot in snapshots {
            scanned += 1;
            let name = snapshot_name(&snapshot).unwrap();
            if !source.contains(&name) {
                orphans.push(format!("  {}", snapshot.display()));
            }
        }
    }

    assert!(
        scanned >= 47,
        "found only {scanned} snapshot files, so the scan cannot be trusted"
    );
    assert!(
        orphans.is_empty(),
        "no test in these crates names these snapshots:\n{}\n\nDelete them, or restore the test.",
        orphans.join("\n")
    );
}

/// All `.rs` source under `src`, concatenated.
fn rust_source(src: &Path) -> String {
    files_under(src)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::render(
        "ui/tui/src/render/snapshots/hub_tui__render__tests__full_screen_merging_pr.snap",
        Some("full_screen_merging_pr")
    )]
    #[case::config(
        "config/src/snapshots/config__toml__tests__snapshot_full_device_config.snap",
        Some("snapshot_full_device_config")
    )]
    #[case::pending(
        "ui/tui/src/render/snapshots/hub_tui__render__tests__new.snap.new",
        None
    )]
    #[case::not_a_snapshot("ui/tui/src/render/mod.rs", None)]
    fn a_snapshot_is_named_by_its_last_segment(#[case] file: &str, #[case] name: Option<&str>) {
        assert_eq!(snapshot_name(Path::new(file)).as_deref(), name);
    }

    #[test]
    fn snapshots_are_found_in_nested_snapshot_directories_only() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("render/snapshots");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("c__render__tests__one.snap"), "").unwrap();
        std::fs::write(nested.join("c__render__tests__two.snap.new"), "").unwrap();
        std::fs::write(dir.path().join("stray.snap"), "").unwrap();

        assert_eq!(
            snapshot_files(dir.path()),
            vec![nested.join("c__render__tests__one.snap")]
        );
    }

    #[test]
    fn source_is_read_from_every_rust_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a")).unwrap();
        std::fs::write(dir.path().join("lib.rs"), "fn one() {}").unwrap();
        std::fs::write(dir.path().join("a/b.rs"), "fn two() {}").unwrap();
        std::fs::write(dir.path().join("a/notes.md"), "fn three() {}").unwrap();

        let source = rust_source(dir.path());

        assert!(source.contains("one") && source.contains("two"), "{source}");
        assert!(!source.contains("three"), "{source}");
    }
}
