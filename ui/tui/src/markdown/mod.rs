// Vendored from tui-markdown 0.3.7 (MIT/Apache-2.0), modified:
//   - SYNTAX_SET uses two-face's extended syntax set (adds TypeScript, TOML, etc.)
//   - Theme uses two-face's built-in CatppuccinMocha instead of base16-ocean.dark
//   - tracing instrumentation removed
//   - pub(crate) visibility throughout
pub(crate) mod options;
pub(crate) mod render;
pub(crate) mod style_sheet;

pub(crate) use render::{from_str, highlight_json};
