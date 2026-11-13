// Vendored from tui-markdown 0.3.7 (MIT/Apache-2.0) and patched. README.md in this directory
// lists its patches and how to re-sync with upstream.
pub(crate) mod options;
pub(crate) mod render;
pub(crate) mod style_sheet;

pub(crate) use render::{from_str, highlight_json};
