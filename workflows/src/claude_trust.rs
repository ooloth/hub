//! Pre-trust Claude Code workspace directories before dispatch.
// pub(crate) fn in a pub(crate) mod triggers redundant_pub_crate (nursery); suppress it.
#![allow(clippy::redundant_pub_crate)]
//!
//! # Safety rationale — why writing to `~/.claude.json` is safe
//!
//! `~/.claude.json` is Claude Code's primary runtime state file, shared across
//! all sessions on this device. Three properties together make this write safe:
//!
//! 1. **`serde_json::Value` is lossless.** Unlike a typed struct (which silently
//!    drops unknown fields on deserialize → serialize round-trip), `Value`
//!    preserves every field it does not understand. The only mutation in this
//!    module is the single key assignment in `set_trust_flag` — nothing else in
//!    the document changes.
//!
//! 2. **Atomic rename.** We write to a `NamedTempFile` in the same directory as
//!    the target, then call `persist()` which resolves to `rename(2)`. If
//!    anything fails before the rename, the original file is untouched. There is
//!    no window in which `~/.claude.json` is empty or partially written.
//!
//! 3. **Soft failure.** If reading or parsing fails (e.g., already-corrupt JSON),
//!    we log a warning and return `Ok(())` from the call site in `dispatch_inner`
//!    — the write is skipped entirely. We cannot make a file worse than it
//!    already is. Stall detection (#281) is the safety net for the case where the
//!    trust prompt blocks the session.

use anyhow::{Context, Result};
use std::path::Path;

/// Marks `workspace` as trusted in `claude_json` before the agent session is
/// spawned. Parameterised for testability; `dispatch()` resolves the real path.
///
/// Returns `Err` on failure — callers should handle with a `warn!` + `Ok(())`.
pub(crate) fn trust_workspace(claude_json: &Path, workspace: &Path) -> Result<()> {
    let doc: serde_json::Value = if claude_json.exists() {
        let raw = std::fs::read_to_string(claude_json)
            .with_context(|| format!("failed to read {}", claude_json.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", claude_json.display()))?
    } else {
        serde_json::Value::Object(serde_json::Map::new())
    };

    let updated = set_trust_flag(doc, workspace)?;

    let parent = claude_json.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .context("failed to create tempfile for claude.json")?;
    serde_json::to_writer_pretty(&mut tmp, &updated)
        .context("failed to serialise updated claude.json")?;
    let _ = tmp
        .persist(claude_json)
        .context("failed to atomically replace claude.json")?;

    Ok(())
}

/// Sets `projects.<workspace_path>.hasTrustDialogAccepted = true` in `doc`.
/// All other fields in the document are preserved unchanged.
fn set_trust_flag(mut doc: serde_json::Value, workspace: &Path) -> Result<serde_json::Value> {
    let path_key = workspace.to_string_lossy().into_owned();

    {
        let root = doc
            .as_object_mut()
            .context("~/.claude.json root is not a JSON object")?;

        let projects = root
            .entry("projects")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

        let projects_obj = projects
            .as_object_mut()
            .context("~/.claude.json 'projects' value is not a JSON object")?;

        let project = projects_obj
            .entry(path_key)
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

        let project_entry = project
            .as_object_mut()
            .context("project entry is not a JSON object")?;

        let _ = project_entry.insert(
            "hasTrustDialogAccepted".into(),
            serde_json::Value::Bool(true),
        );
    }

    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const WS_PATH: &str = "/Users/michael/.hub/workspaces/TASK-0001/hub";

    fn workspace() -> &'static Path {
        Path::new(WS_PATH)
    }

    // ── set_trust_flag (pure unit tests) ─────────────────────────────────────

    #[test]
    fn set_trust_flag_inserts_projects_key_when_absent() {
        let doc = serde_json::json!({ "theme": "dark" });
        let result = set_trust_flag(doc, workspace()).unwrap();
        assert_eq!(result["projects"][WS_PATH]["hasTrustDialogAccepted"], true);
    }

    #[test]
    fn set_trust_flag_inserts_project_entry_when_path_absent() {
        let doc = serde_json::json!({
            "projects": {
                "/other/path": { "hasTrustDialogAccepted": true }
            }
        });
        let result = set_trust_flag(doc, workspace()).unwrap();
        assert_eq!(result["projects"][WS_PATH]["hasTrustDialogAccepted"], true);
        // existing entry must be preserved
        assert_eq!(
            result["projects"]["/other/path"]["hasTrustDialogAccepted"],
            true
        );
    }

    #[test]
    fn set_trust_flag_preserves_existing_project_fields() {
        let doc = serde_json::json!({
            "projects": {
                WS_PATH: {
                    "hasTrustDialogAccepted": false,
                    "allowedTools": ["read", "write"],
                    "mcpServers": {}
                }
            }
        });
        let result = set_trust_flag(doc, workspace()).unwrap();
        let entry = &result["projects"][WS_PATH];
        assert_eq!(entry["hasTrustDialogAccepted"], true);
        assert_eq!(entry["allowedTools"], serde_json::json!(["read", "write"]));
        assert_eq!(entry["mcpServers"], serde_json::json!({}));
    }

    #[test]
    fn set_trust_flag_is_idempotent_when_already_true() {
        let doc = serde_json::json!({
            "projects": { WS_PATH: { "hasTrustDialogAccepted": true } }
        });
        let result = set_trust_flag(doc, workspace()).unwrap();
        assert_eq!(result["projects"][WS_PATH]["hasTrustDialogAccepted"], true);
    }

    #[test]
    fn set_trust_flag_errors_when_root_is_not_object() {
        let result = set_trust_flag(serde_json::json!(null), workspace());
        assert!(result.is_err());
    }

    /// Snapshot guard: verifies that `set_trust_flag` touches exactly one key
    /// in a realistic full-document fixture. Any unintended field mutations will
    /// cause this snapshot to fail.
    #[test]
    fn set_trust_flag_does_not_alter_any_other_field() {
        let fixture = serde_json::json!({
            "numStartups": 42,
            "installMethod": "manual",
            "theme": "dark",
            "hasCompletedOnboarding": true,
            "migrationVersion": 3,
            "showSpinnerTree": true,
            "mcpServers": {},
            "githubRepoPaths": ["/Users/michael/Repos"],
            "projects": {
                "/Users/michael/Repos/ooloth/dotfiles": {
                    "allowedTools": [],
                    "hasTrustDialogAccepted": true,
                    "mcpServers": {},
                    "lastCost": 1.23,
                    "exampleFiles": ["aliases.zsh", "Brewfile"]
                },
                WS_PATH: {
                    "allowedTools": [],
                    "hasTrustDialogAccepted": false,
                    "mcpServers": {
                        "linear-server": {
                            "type": "http",
                            "url": "https://mcp.linear.app/mcp"
                        }
                    }
                }
            },
            "tipLifetimeShownCounts": {}
        });
        let result = set_trust_flag(fixture, workspace()).unwrap();
        insta::assert_json_snapshot!(result);
    }

    // ── trust_workspace (integration tests) ──────────────────────────────────

    #[test]
    fn trust_workspace_creates_file_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("claude.json");

        trust_workspace(&json_path, workspace()).unwrap();

        let content = std::fs::read_to_string(&json_path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(doc["projects"][WS_PATH]["hasTrustDialogAccepted"], true);
    }

    #[test]
    fn trust_workspace_merges_into_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("claude.json");
        let existing = serde_json::json!({
            "theme": "dark",
            "projects": {
                "/other/project": { "hasTrustDialogAccepted": true, "lastCost": 9.99 }
            }
        });
        std::fs::write(&json_path, serde_json::to_string_pretty(&existing).unwrap()).unwrap();

        trust_workspace(&json_path, workspace()).unwrap();

        let content = std::fs::read_to_string(&json_path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(doc["projects"][WS_PATH]["hasTrustDialogAccepted"], true);
        assert_eq!(
            doc["projects"]["/other/project"]["hasTrustDialogAccepted"],
            true
        );
        assert_eq!(doc["projects"]["/other/project"]["lastCost"], 9.99);
        assert_eq!(doc["theme"], "dark");
    }

    #[test]
    fn trust_workspace_errors_on_malformed_json_and_leaves_file_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("claude.json");
        let bad_content = b"not valid json {{{";
        std::fs::write(&json_path, bad_content).unwrap();

        let result = trust_workspace(&json_path, workspace());
        assert!(result.is_err(), "must error on malformed JSON");
        // original file must be untouched (atomic rename: no partial write)
        assert_eq!(std::fs::read(&json_path).unwrap(), bad_content);
    }
}
