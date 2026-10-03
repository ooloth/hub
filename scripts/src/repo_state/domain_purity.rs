use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::repo_root::repo_root;

/// The known ways code reaches the environment, the filesystem, a process, a clock or a
/// random source. Names, not meaning: see docs/invariants/domain-is-pure.md for what this misses.
const AMBIENT_STATE: &[&str] = &[
    "std::env",
    "std::fs",
    "std::process",
    "SystemTime",
    "Instant::now",
    "Utc::now",
    "Local::now",
    "rand::",
    "thread_rng",
];

/// A line of source that mentions one of the known ways of reaching ambient state.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AmbientRead {
    /// 1-based.
    line: usize,
    pattern: &'static str,
    text: String,
}

impl AmbientRead {
    /// Every line of `source` that mentions a known way of reaching ambient state.
    fn find_in(source: &str) -> Vec<Self> {
        source
            .lines()
            .enumerate()
            .flat_map(|(index, text)| {
                AMBIENT_STATE
                    .iter()
                    .filter(move |pattern| text.contains(**pattern))
                    .map(move |pattern| Self {
                        line: index + 1,
                        pattern,
                        text: text.to_string(),
                    })
            })
            .collect()
    }
}

/// Every `.rs` file under `dir`, at any depth, sorted.
///
/// # Errors
/// Returns an error when a directory cannot be read.
fn rust_files_under(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("failed to read {}", current.display()))?;
        for entry in entries {
            let path = entry
                .with_context(|| format!("failed to read an entry in {}", current.display()))?
                .path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// A domain type that reads a clock, the environment, a file or a random source is no longer a
/// pure function of its inputs, so it cannot be built identically in a test, the TUI, a workflow
/// and the daemon. See docs/invariants/domain-is-pure.md
#[test]
fn domain_reads_no_ambient_state() {
    let domain_src = repo_root().join("domain/src");
    let files = rust_files_under(&domain_src).unwrap();
    assert!(
        files.contains(&domain_src.join("lib.rs")),
        "found no domain/src/lib.rs under {}, so nothing was scanned",
        domain_src.display()
    );

    let reads: Vec<String> = files
        .iter()
        .flat_map(|file| {
            let source = std::fs::read_to_string(file)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", file.display()));
            AmbientRead::find_in(&source).into_iter().map(move |read| {
                format!(
                    "  {}:{}: {} ({})",
                    file.display(),
                    read.line,
                    read.text.trim(),
                    read.pattern
                )
            })
        })
        .collect();

    assert!(
        reads.is_empty(),
        "domain/ must not read ambient state, but does:\n{}\n\n\
         Take the value as an argument instead. See docs/invariants/domain-is-pure.md",
        reads.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::env("let home = std::env::var(\"HOME\");", "std::env")]
    #[case::fs("let s = std::fs::read_to_string(p);", "std::fs")]
    #[case::process("std::process::Command::new(\"git\")", "std::process")]
    #[case::system_time("let t = SystemTime::now();", "SystemTime")]
    #[case::instant("let t = Instant::now();", "Instant::now")]
    #[case::utc("let t = Utc::now();", "Utc::now")]
    #[case::local("let t = Local::now();", "Local::now")]
    #[case::rand("let n: u8 = rand::random();", "rand::")]
    #[case::thread_rng("let mut r = thread_rng();", "thread_rng")]
    fn each_known_way_in_is_found(#[case] line: &str, #[case] pattern: &str) {
        let source = format!("fn f() {{\n    {line}\n}}\n");

        let reads = AmbientRead::find_in(&source);

        assert_eq!(
            reads
                .iter()
                .map(|read| (read.line, read.pattern))
                .collect::<Vec<_>>(),
            vec![(2, pattern)]
        );
    }

    #[test]
    fn receiving_a_value_as_an_argument_is_allowed() {
        let source = "use std::path::Path;\nfn f(now: DateTime<Utc>, dir: &Path) {}\n";

        assert!(AmbientRead::find_in(source).is_empty());
    }

    #[test]
    fn files_are_found_at_any_depth() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("lib.rs"), "").unwrap();
        std::fs::write(dir.path().join("nested/deep.rs"), "").unwrap();
        std::fs::write(dir.path().join("notes.md"), "").unwrap();

        let files = rust_files_under(dir.path()).unwrap();

        assert_eq!(
            files,
            vec![dir.path().join("lib.rs"), dir.path().join("nested/deep.rs")]
        );
    }
}
