# hub

A personal command center: surfaces what needs attention today across software I'm responsible for, helps prioritize what to act on.

## What This Is

- **Local-only** — runs on each device; each has its own SQLite db
- **Context-aware** — work laptop shows work software; personal laptop shows personal software
- **Extensible** — adding a new integration = adding files to `clients/` and `workflows/`; no registration step
- **Rust** — single binary, CLI entry point today, TUI entry point later

## Stack

| Concern | Choice |
|---|---|
| Language | Rust |
| Async runtime | tokio |
| CLI | clap (derive) |
| TUI | ratatui (planned) |
| HTTP clients | reqwest |
| SQLite | rusqlite (bundled) or sqlx |
| Serialization | serde |
| Config files | toml |
| Error handling | anyhow |

## Project Structure

```
clients/     # external API wrappers — one subdir per service
config/      # parses ~/.hub/config.toml into domain types
domain/      # types + pure logic; no I/O; no imports from other hub crates
store/       # local SQLite reads/writes
workflows/   # orchestrated operations; the "what hub does"
ui/
  cli/       # hub binary — bootstraps config, wires deps, calls workflows
  tui/       # hub-tui binary (planned)
scripts/     # dev/ops scripts; not part of the binary
docs/        # architecture, conventions, decisions
```

Import direction (never import rightward's left neighbor):
```
ui/ → workflows/ → clients/ → domain/
                 → store/   → domain/
     config/               → domain/
```

## Rust Conventions

See `docs/conventions.md` for full rationale. Hard rules for agents:

- **Error handling**: `anyhow` only. No `thiserror`. `?` everywhere. `.context("msg")` for human-readable chains.
- **Owned types**: structs hold `String`/`Vec<T>`. Functions that only read take `&str`/`&[T]`. Return owned values, not references.
- **No lifetime annotations**: if you're writing `'a`, stop and restructure. Return owned types instead.
- **Clone freely**: don't fight the borrow checker. Clone across `.await` points. Optimize only if profiling shows it matters.
- **Async**: `#[tokio::main]`, `features = ["full"]`. Use `tokio::join!` for parallel work. Use `tokio::fs`/`tokio::time` not std equivalents inside async.
- **Config**: plain `toml::from_str` into typed structs. `std::env::var` inline for overrides. No `figment` or `config` crate.
- **CLI**: `clap` with derive macros. Annotate structs; don't use the builder API.

## Development

```bash
cargo check          # verify workspace compiles
cargo build          # build all crates
cargo run -p hub-cli # run the CLI
cargo test           # run all tests
```
