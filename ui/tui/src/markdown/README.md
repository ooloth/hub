# markdown

Vendored and patched fork of [tui-markdown 0.3.7][tui-markdown] (MIT/Apache-2.0).

## What changed

These changes in `render.rs` differ from upstream:

1. **`SYNTAX_SET`** — uses `two_face::syntax::extra_newlines()` instead of
   `SyntaxSet::load_defaults_newlines()`. This extends the bundled syntax set from
   syntect's default ~75 Sublime Text 2 languages to 150+ modern languages, adding
   TypeScript, TOML, and others that were missing.

2. **`MOCHA_THEME`** — uses `two_face::theme::extra().get(EmbeddedThemeName::CatppuccinMocha)`
   instead of `base16-ocean.dark`.

3. **`text`** — splits multi-line text with `enumerate()` where upstream uses
   `itertools::with_position()`, so hub-tui does not depend on `itertools` directly.

4. **Loose lists** — a list item's first paragraph continues the bullet's line, which
   `start_item` marks with `bullet_line_awaits_content`. Upstream starts the paragraph on a new
   line, so a loose list renders each bullet alone above its text, and a loose task list panics
   in `task_list_marker` because the marker arrives on an empty line. `task_list_marker` also
   clamps its fallback insert to the line's span count.

5. **Unlanguaged code fences** — a fence with no language, or one syntect does not know, carries
   the stylesheet's `code()` style on its content lines, and the fences themselves stay unstyled.

6. **`highlight_json`** — added for the log detail screen (`render/log.rs`). It is not in
   upstream, so a re-sync keeps it.

`style_sheet.rs` adds `CatppuccinStyleSheet`, which the module's `from_str` uses
as its default. It styles headings in Mocha teal and inline code in Mocha lavender.

`tracing` instrumentation (`#[instrument]`, `debug!`, `warn!`) is stripped, so the
module does not depend on `tracing` directly.

## Staying in sync with upstream

[tui-markdown releases][releases] are infrequent. If a new version ships, diff
`src/lib.rs` from upstream against `render.rs` here, apply any logic changes, and
re-apply the patches above.

[tui-markdown]: https://github.com/joshka/tui-markdown
[releases]: https://github.com/joshka/tui-markdown/releases
