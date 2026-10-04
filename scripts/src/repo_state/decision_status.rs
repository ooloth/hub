use std::path::PathBuf;

use crate::repo_root::repo_root;

/// Whether a decision record is both superseded and still marked as a design to build.
///
/// AGENTS.md treats `rg "not yet implemented" docs/decisions/` as the list of settled designs
/// still to build, so a superseded record carrying that phrase sends the next agent to build
/// something that was abandoned. A record counts as superseded when a blockquote banner line says
/// "Superseded" or its frontmatter status starts with "superseded".
fn superseded_but_pending(record: &str) -> bool {
    let superseded = record.lines().any(|line| {
        let line = line.trim_start();
        (line.starts_with('>') && line.contains("Superseded"))
            || line.to_lowercase().starts_with("status: superseded")
    });
    superseded && record.contains("not yet implemented")
}

#[test]
fn no_superseded_decision_is_listed_as_pending() {
    let dir = repo_root().join("docs/decisions");
    let mut records: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .filter(|path| path.file_name().is_some_and(|name| name != "README.md"))
        .collect();
    records.sort();
    assert!(
        records.len() >= 25,
        "found {} decision records in {}, so the scan cannot be trusted",
        records.len(),
        dir.display()
    );

    let pending: Vec<String> = records
        .iter()
        .filter(|path| superseded_but_pending(&std::fs::read_to_string(path).unwrap()))
        .map(|path| format!("  {}", path.display()))
        .collect();

    assert!(
        pending.is_empty(),
        "these decision records are superseded but still say \"not yet implemented\", so they read \
         as designs to build:\n{}\n\nSay instead that the record is superseded and not to be built.",
        pending.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::banner_and_pending(
        "# 016\n\n> **⚠ Superseded by Decision 019.**\n\n_Status: accepted; not yet implemented._\n",
        true
    )]
    #[case::frontmatter_and_pending(
        "---\nstatus: superseded by 030\n---\n\n_Status: accepted; not yet implemented._\n",
        true
    )]
    #[case::superseded_only("# 008\n\n> **⚠ Superseded by Decision 021.**\n", false)]
    #[case::pending_only("# 021\n\n_Status: accepted; not yet implemented._\n", false)]
    #[case::partially_reversed(
        "# 009\n\n> **⚠ Partially reversed by Decision 012.**\n\n_Status: not yet implemented._\n",
        false
    )]
    #[case::superseded_mentioned_in_prose(
        "# 022\n\nThis record superseded nothing.\n\n_Status: not yet implemented._\n",
        false
    )]
    fn a_record_is_flagged_only_when_superseded_and_pending(
        #[case] record: &str,
        #[case] flagged: bool,
    ) {
        assert_eq!(superseded_but_pending(record), flagged);
    }
}
