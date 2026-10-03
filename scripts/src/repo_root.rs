use std::path::{Path, PathBuf};

/// The root of the hub checkout this binary was built from: the directory above this crate.
///
/// Compile-time on purpose. A worktree builds its own binary, so a tool run there acts on that
/// worktree rather than on whichever checkout the shell happens to be in.
pub(crate) fn repo_root() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_dir.parent().unwrap_or(crate_dir).to_path_buf()
}
