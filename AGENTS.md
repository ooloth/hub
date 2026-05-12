# hub

## Private constraints

If `../hub-private/CLAUDE.md` exists, read it before writing or editing any files — it lists terms that must never appear in committed hub files.

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
  tui/       # hub-tui binary
scripts/     # dev/ops scripts; not part of the binary
docs/        # architecture, decisions, playbooks
```

Import direction (never import rightward's left neighbor):

```
ui/ → config/              → domain/
   → workflows/ → clients/ → domain/
                → store/   → domain/
```

`config/` is a direct dependency of `ui/cli` and `ui/tui`. Config values
are passed as function arguments into workflows and clients — those crates
do not depend on `config/` directly.

## Stack

| Concern        | Choice                     |
| -------------- | -------------------------- |
| Language       | Rust                       |
| Async runtime  | tokio                      |
| CLI            | clap (derive)              |
| TUI            | ratatui                    |
| HTTP clients   | reqwest                    |
| SQLite         | rusqlite (bundled) or sqlx |
| Serialization  | serde                      |
| Secrets        | 1Password CLI (`op run`)   |
| Error handling | anyhow                     |

### Rust Conventions

See `~/.claude/references/rust.md` to understand this project's preference for Easy Mode Rust.

Hard rules for agents:

- **Error handling**: `anyhow` only. No `thiserror`. `?` everywhere. `.context("msg")` for human-readable chains.
- **Owned types**: structs hold `String`/`Vec<T>`. Functions that only read take `&str`/`&[T]`. Return owned values, not references.
- **No lifetime annotations**: if you're writing `'a`, stop and restructure. Return owned types instead.
- **Clone freely**: don't fight the borrow checker. Clone across `.await` points. Optimize only if profiling shows it matters.
- **Async**: `#[tokio::main]`, `features = ["full"]`. Use `tokio::join!` for parallel work. Use `tokio::fs`/`tokio::time` not std equivalents inside async.
- **Secrets**: read from env vars via `std::env::var`. Never read from files. Injected at runtime by `op run --env-file=.env`.
- **CLI**: `clap` with derive macros. Annotate structs; don't use the builder API.

### Schema versioning

When adding, removing, or changing any field on a `#[derive(Serialize, Deserialize)]` domain type, check `ui/tui/README.md` to determine whether `SCHEMA_VERSION` in `workflows/src/status.rs` needs a bump. Bump it before committing — the rules for when a bump is required are in that file.

## Development

```bash
just check   # fmt + lint (autofixes where possible)
just test    # run all tests
just build   # build all crates
just cli     # run the CLI
just tui     # run the TUI
```

### Verifying TUI changes

TUI verification has two tiers depending on what changed.

**Tier 1 — snapshot tests (rendering and layout changes)**

Full-screen `insta` snapshots cover all major screen states (see
`ui/tui/README.md` for the full list and conventions). If a rendering
change causes a visual regression, a snapshot diff will show exactly what
changed. Run `just test` and review any failures.

If the diff is intentional, accept it:

```bash
INSTA_UPDATE=always cargo test -p hub-tui
```

When adding a new screen state or item type, add a snapshot for it —
don't rely on the existing snapshots to catch regressions in new code
paths.

**Tier 2 — tmux E2E (interaction and behavior changes)**

For changes that affect keybindings, navigation between screens,
subprocess launching, tmux integration, store schema, cache format, or
domain types the TUI deserializes on startup, snapshots are not sufficient.
Run the TUI live in tmux and drive the interaction:

1. Start the TUI in tmux.
2. Drive the changed keybinding or interaction with `tmux send-keys`.
3. Capture the pane with `tmux capture-pane -p`.
4. If the change launches another pane, window, browser, shell command, or
   external process, verify that launch behavior live.
5. Clean up any tmux panes/windows created during the test.
6. Report exactly what was observed.

```bash
tmux new-window -n "tui-test" "just tui; read"
sleep 3                                          # wait for data to load
tmux send-keys -t "tui-test" "?" ""             # send a keystroke
sleep 0.5
tmux capture-pane -t "tui-test" -p              # read the screen
tmux kill-window -t "tui-test"                  # clean up
```

**tmux send-keys pitfalls**

- Use named keys for special keys: `"Enter"`, `"Escape"`, `"Backspace"`,
  `"Up"`, `"Down"`, `"Tab"`. An empty string `""` sends **nothing** — it is
  not a shorthand for Enter.
- When testing the filter query flow: commit the query with `"Enter"` before
  pressing `"/"` again. If the query is not committed the TUI stays in query
  mode and the second `"/"` is treated as `AppendQuery('/')`, not `StartQuery`.

If E2E validation cannot be run, explicitly state why and what weaker
validation was run instead.

## Docs by Area

### Conventions and architecture

| Doc                                      | Covers                                                |
| ---------------------------------------- | ----------------------------------------------------- |
| `docs/architecture/secrets.md`           | 1Password → op run → env var model                    |
| `docs/architecture/private-workflows.md` | Two-repo model for private workflows                  |
| `clients/README.md`                      | reqwest pattern for HTTP clients                      |
| `store/README.md`                        | rusqlite pattern, db path, Connection threading notes |
| `ui/cli/README.md`                       | clap derive API for CLI commands                      |
| `ui/tui/README.md`                       | TUI architecture, cache/schema version, keybindings   |

### Playbooks

| Doc                                                     | Covers                                      |
| ------------------------------------------------------- | ------------------------------------------- |
| `docs/playbooks/add-a-workflow.md`                      | Adding a new workflow end to end            |
| `docs/playbooks/add-a-prompt.md`                        | Adding a Claude Code investigation prompt   |
| `docs/playbooks/add-a-project.md`                       | Adding a project to a device config         |
| `docs/playbooks/add-a-private-workflow.md`              | Adding a workflow to hub-private            |
| `docs/playbooks/set-up-private-workflows-repository.md` | First-time or recovery setup of hub-private |

## File relationships

- `AGENTS.md` and `CLAUDE.md` are symlinked
- `.agents/skills/` and `.claude/skills/` are symlinked

## Directory notes

`.agents/` is the agent harness tracking directory — it holds skill files and
session state for agent runs. It is not a Rust crate. The `agents/` Rust crate
for background automation is described in Decision 005 but has not been built yet.
