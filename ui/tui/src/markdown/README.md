# markdown

Vendored and patched fork of [tui-markdown 0.3.7][tui-markdown] (MIT/Apache-2.0).

## What changed

Two lines in `mod.rs` differ from upstream:

1. **`SYNTAX_SET`** — uses `two_face::syntax::extra_newlines()` instead of
   `SyntaxSet::load_defaults_newlines()`. This extends the bundled syntax set from
   syntect's default ~75 Sublime Text 2 languages to 150+ modern languages, adding
   TypeScript, TOML, and others that were missing.

2. **`MOCHA_THEME`** — uses `two_face::theme::extra().get(EmbeddedThemeName::CatppuccinMocha)`
   instead of `base16-ocean.dark`.

`style_sheet.rs` adds `CatppuccinStyleSheet`, which the module's `from_str` uses
as its default. It styles headings and inline code in Mocha mauve to match
`FOCUS_COLOR` in `render/mod.rs`.

`tracing` instrumentation (`#[instrument]`, `debug!`, `warn!`) is stripped to
avoid the `tracing` dependency.

## Staying in sync with upstream

[tui-markdown releases][releases] are infrequent. If a new version ships, diff
`src/lib.rs` from upstream against `mod.rs` here, apply any logic changes, and
re-apply the two patches above.

[tui-markdown]: https://github.com/joshka/tui-markdown
[releases]: https://github.com/joshka/tui-markdown/releases
