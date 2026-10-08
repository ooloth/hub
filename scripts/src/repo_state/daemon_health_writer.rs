use std::path::{Path, PathBuf};

use super::domain_purity::rust_files_under;
use super::workspace::WorkspaceMember;
use crate::repo_root::repo_root;

/// The table whose only writer is `store::daemon_health::record`.
const TABLE: &str = "daemon_health";

/// The file that owns the table, and the only one allowed to name it in SQL.
const OWNER: &str = "store/src/daemon_health.rs";

/// Each line of `source` that names the table itself rather than the store module that owns it.
///
/// `store::daemon_health::record` and `mod daemon_health;` name the module; anything else, such as
/// `INSERT INTO daemon_health`, names the table.
fn table_mentions(source: &str) -> Vec<(usize, String)> {
    source
        .lines()
        .enumerate()
        .filter(|(_, text)| {
            text.match_indices(TABLE).any(|(start, _)| {
                let before = text[..start].trim_end();
                let after = &text[start + TABLE.len()..];
                !after.starts_with("::") && !before.ends_with("mod")
            })
        })
        .map(|(index, text)| (index + 1, text.trim().to_string()))
        .collect()
}

/// A second writer of the health record could write it outside the transaction that writes the
/// payload, and a reader would then see one pass's payload beside another pass's health. See
/// Decision 026 and the design for #341.
#[test]
fn only_the_store_names_the_daemon_health_table() {
    let root = repo_root();
    let exempt: Vec<PathBuf> = vec![
        root.join(OWNER),
        root.join("scripts/src/repo_state/daemon_health_writer.rs"),
    ];
    let members = WorkspaceMember::of_this_repo().unwrap();
    let files: Vec<PathBuf> = members
        .iter()
        .flat_map(|member| rust_files_under(member.dir()).unwrap())
        .filter(|file| !exempt.contains(file))
        .collect();
    assert!(
        files
            .iter()
            .any(|file| file.ends_with("daemon/src/cache.rs")),
        "the scan did not reach daemon/src/cache.rs, so it cannot be trusted"
    );

    let mentions: Vec<String> = files
        .iter()
        .flat_map(|file| {
            let source = std::fs::read_to_string(file)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", file.display()));
            table_mentions(&source)
                .into_iter()
                .map(move |(line, text)| format!("  {}:{line}: {text}", relative(file)))
        })
        .collect();

    assert!(
        mentions.is_empty(),
        "only {OWNER} may name the {TABLE} table, but these do:\n{}\n\n\
         Write the health record through store::daemon_health::record.",
        mentions.join("\n")
    );
}

fn relative(file: &Path) -> String {
    file.strip_prefix(repo_root())
        .unwrap_or(file)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::an_insert("conn.execute(\"INSERT INTO daemon_health (id) VALUES (1)\", [])")]
    #[case::an_update("\"UPDATE daemon_health SET pid = 1\"")]
    #[case::a_drop("conn.execute_batch(\"DROP TABLE daemon_health\")")]
    fn naming_the_table_is_found(#[case] line: &str) {
        let source = format!("fn f() {{\n    {line}\n}}\n");

        assert_eq!(table_mentions(&source), vec![(2, line.to_string())]);
    }

    #[rstest]
    #[case::a_call("store::daemon_health::record(&conn, &pass, None)")]
    #[case::a_module("pub mod daemon_health;")]
    #[case::an_import("use store::daemon_health::RecordedPass;")]
    fn naming_the_module_is_allowed(#[case] line: &str) {
        assert!(table_mentions(line).is_empty(), "{line}");
    }
}
