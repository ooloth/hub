# hub

## What This Is

See [README.md](README.md) for the full feature list and value proposition.

## Project Structure

```
clients/     # external API wrappers — one subdir per service
config/      # reads env vars into typed domain structs
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

## Stack

| Concern        | Choice                     |
| -------------- | -------------------------- |
| Language       | Rust                       |
| Async runtime  | tokio                      |
| CLI            | clap (derive)              |
| TUI            | ratatui (planned)          |
| HTTP clients   | reqwest                    |
| SQLite         | rusqlite (bundled) or sqlx |
| Serialization  | serde                      |
| Secrets        | 1Password CLI (`op run`)   |
| Error handling | anyhow                     |

### Rust Conventions

See [docs/conventions/code-style.md](/docs/conventions/code-style.md) to understand this project's preference for Easy Mode Rust. Hard rules for agents:

- **Error handling**: `anyhow` only. No `thiserror`. `?` everywhere. `.context("msg")` for human-readable chains.
- **Owned types**: structs hold `String`/`Vec<T>`. Functions that only read take `&str`/`&[T]`. Return owned values, not references.
- **No lifetime annotations**: if you're writing `'a`, stop and restructure. Return owned types instead.
- **Clone freely**: don't fight the borrow checker. Clone across `.await` points. Optimize only if profiling shows it matters.
- **Async**: `#[tokio::main]`, `features = ["full"]`. Use `tokio::join!` for parallel work. Use `tokio::fs`/`tokio::time` not std equivalents inside async.
- **Secrets**: read from env vars via `std::env::var`. Never read from files. Injected at runtime by `op run --env-file=.env`.
- **CLI**: `clap` with derive macros. Annotate structs; don't use the builder API.

## Development

```bash
just check   # fmt + lint (autofixes where possible)
just test    # run all tests
just build   # build all crates
just cli     # run the CLI
just tui     # run the TUI
```

## Docs by Area

### Conventions and architecture

| Doc                                      | Covers                                                       |
| ---------------------------------------- | ------------------------------------------------------------ |
| `docs/conventions/code-style.md`         | Easy Mode Rust — error handling, owned types, cloning, async |
| `docs/conventions/assertions.md`         | When to use `assert!` vs `Result`                            |
| `docs/conventions/testing.md`            | Unit tests, rstest, insta, proptest, cargo-mutants           |
| `docs/architecture/secrets.md`           | 1Password → op run → env var model                           |
| `docs/architecture/private-workflows.md` | Two-repo model for private workflows                         |
| `clients/README.md`                      | reqwest pattern for HTTP clients                             |
| `store/README.md`                        | rusqlite pattern, db path, Connection threading notes        |
| `ui/cli/README.md`                       | clap derive API for CLI commands                             |
| `ui/tui/README.md`                       | TUI architecture, cache/schema version, keybindings          |

### Playbooks

| Doc                                                     | Covers                                      |
| ------------------------------------------------------- | ------------------------------------------- |
| `docs/playbooks/add-a-workflow.md`                      | Adding a new workflow end to end            |
| `docs/playbooks/add-a-skill.md`                         | Adding a Claude Code investigation skill    |
| `docs/playbooks/add-a-project.md`                       | Adding a project to a device config         |
| `docs/playbooks/add-a-private-workflow.md`              | Adding a workflow to hub-private            |
| `docs/playbooks/set-up-private-workflows-repository.md` | First-time or recovery setup of hub-private |

## File relationships

- `AGENTS.md` and `CLAUDE.md` are symlinked
- `.agents/skills/` and `.claude/skills/` are symlinked
