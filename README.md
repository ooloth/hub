# hub

Personal command center. Surfaces what needs attention today across software I'm responsible for.

- Local-only — each device has its own SQLite database
- Context-aware — work laptop tracks work software; personal laptop tracks personal software
- Rust — single binary, CLI today, TUI planned

## Prerequisites

- [Rust](https://rustup.rs)
- [just](https://github.com/casey/just) — `brew install just`
- [1Password CLI](https://developer.1password.com/docs/cli) — `brew install 1password-cli`

## Setup

```bash
git clone <repo> && cd hub
cp .env.example .env
# edit .env — replace op:// references with your actual 1Password paths
just check
```

## Running

```bash
just check              # verify workspace compiles
op run --env-file=.env -- just status   # run with secrets injected
```

## Docs

- [Vision](docs/vision.md) — what this is, why, and where it's going
- [Decisions](docs/decisions/) — architectural decisions and their rationale
- [Conventions](docs/conventions.md) — Rust patterns used throughout
