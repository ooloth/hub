# hub

Personal command center. Surfaces what needs attention today across software I'm responsible for.

- Local-only — each device has its own SQLite database
- Context-aware — work laptop tracks work software; personal laptop tracks personal software
- Rust — single binary, CLI today, TUI planned

## Quickstart

```bash
cargo build
cargo run -p hub-cli
```

## Docs

- [Conventions](docs/conventions.md) — Rust patterns used throughout
