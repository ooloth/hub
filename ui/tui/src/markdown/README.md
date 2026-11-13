# markdown

Vendored and patched fork of [tui-markdown 0.3.7][tui-markdown] (MIT/Apache-2.0).

## What changed

These changes in this module differ from upstream:

1. **`SYNTAX_SET`**. Uses `two_face::syntax::extra_newlines()` instead of
   `SyntaxSet::load_defaults_newlines()`. This extends the bundled syntax set from
   syntect's default ~75 Sublime Text 2 languages to 150+ modern languages, adding
   TypeScript, TOML, and others that were missing.

2. **`MOCHA_THEME`**. Uses `two_face::theme::extra().get(EmbeddedThemeName::CatppuccinMocha)`
   instead of `base16-ocean.dark`.

3. **`text`**. Splits multi-line text with `enumerate()` where upstream uses
   `itertools::with_position()`, so hub-tui does not depend on `itertools` directly.

4. **Loose lists**. A list item's first paragraph continues the bullet's line. `start_item`
   sets `bullet_line_awaits_content`, and `handle_event` clears it before each event, so only a
   paragraph that starts straight after the bullet joins it. In 0.3.7 the paragraph starts on a
   new line, so a loose list renders each bullet alone above its text, and a loose task list
   panics in `task_list_marker` because the marker arrives on an empty line. `task_list_marker`
   also clamps its fallback insert to the line's span count. Upstream fixed loose lists in
   0.3.9 ([#166][upstream-166]) and added a matching clamp in 0.3.11 ([#207][upstream-207]), so a
   re-sync to 0.3.11 or later takes upstream's fix in place of this one. The list tests in
   `render.rs` must still pass after it.

5. **Unlanguaged code fences**. A fence with no language, or one syntect does not know, carries
   the stylesheet's `code()` style on its content lines, and the fences themselves stay unstyled.

6. **`highlight_json`**. Added for the log detail screen (`render/log.rs`). It is not in
   upstream, so a re-sync keeps it.

7. **Superscript and subscript**. `from_str` does not turn on `ENABLE_SUPERSCRIPT` or
   `ENABLE_SUBSCRIPT`, so neither is parsed, though the renderer still handles both tags.

8. **Syntax highlighting is always on**. Upstream's `highlight-code` cargo feature is removed,
   and the syntect highlighting it gated always compiles in.

`style_sheet.rs` adds `CatppuccinStyleSheet`, which the module's `from_str` uses
as its default. It styles headings in Mocha teal and inline code in Mocha lavender.

`tracing` instrumentation (`#[instrument]`, `debug!`, `warn!`) is stripped, so the
module does not depend on `tracing` directly.

## Staying in sync with upstream

When a new [tui-markdown release][releases] ships, diff upstream's `tui-markdown/src/` against
this module, apply any logic changes, and re-apply the patches above. Upstream is a workspace,
and since 0.3.9 its renderer is split across `tui-markdown/src/renderer/`, with highlighting
setup in `tui-markdown/src/code_theme.rs`, so a patch may land in a different file from the one
named here.

[tui-markdown]: https://github.com/joshka/tui-markdown
[releases]: https://github.com/joshka/tui-markdown/releases
[upstream-166]: https://github.com/joshka/tui-markdown/pull/166
[upstream-207]: https://github.com/joshka/tui-markdown/pull/207
