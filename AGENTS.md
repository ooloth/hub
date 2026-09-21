# hub

## Private constraints

If `../hub-private/CLAUDE.md` exists, read it before writing or editing any files — it lists terms that must never appear in committed hub files.

## What This Is

Hub is a personal command center that aggregates signals from multiple sources — GitHub PRs, CI status, Loki alerts, Linear issues, and more via the `private` feature — into a single urgency-ranked terminal view, and delegates action on those signals to agents via filesystem-based investigation sessions.

Its three binaries serve three distinct audiences:

- **`hub-tui`** (Ratatui dashboard) — the **human-facing surface**. Read signals, launch investigation sessions, watch session progress, review results.
- **`hub`** (CLI) — the **agent's toolkit** (stub). The task subcommand (`hub task *`) was removed with the task model (ADR 019). Future agent-facing subcommands will be added here as the filesystem session model is built out.
- **`hub-daemon`** — the **unattended surface** (ADR 020). Refreshes the signal cache with nobody present. Today it runs one refresh and exits; looping, health reporting, credential retry, launchd and notifications are the rest of the [#327](https://github.com/ooloth/hub/issues/327) milestone.

The core value is cross-domain triage plus agent delegation: signals from different systems are ranked together in one list, and any signal can be investigated by pressing `i` to launch a Claude Code session with injected context. That session opens in its own tmux window, named after the signal by `domain::InvestigationWindow`, so several investigations run side by side and each stays reachable from tmux's window list.

See [README.md](README.md) for the full feature list and value proposition.

## Active milestone

Work in flight is tracked in the GitHub milestone **Granular PR queue notifications**, epic
[#327](https://github.com/ooloth/hub/issues/327). Before starting anything in it, read the epic's
plan of record:

```bash
gh issue view 327 --comments
```

The plan lives in a **comment**. `gh issue view 327` on its own prints the body and stops, and the
body predates the plan on several points, so reading it alone gives you a superseded design. The
comment carries the settled architecture, the measured spike findings, and the full phase sequence.

**The title numbering is the work order.** Issues are titled `Phase N.M — ...`; take them in that
order. Hard dependencies are recorded separately as GitHub `blockedBy` relationships, readable only
via GraphQL, and there is currently exactly one (#331 needs #330). Everything else the numbering
implies is sequence, not blocking.

## Project Structure

```
clients/     # external API wrappers — one file (or subdirectory) per external service
config/      # reads hub.toml and resolves credentials into typed domain structs
daemon/      # hub-daemon binary — refreshes the cache with nobody present
domain/      # types + pure logic; no I/O; no imports from other hub crates
store/       # local SQLite reads/writes
workflows/   # orchestrated operations; the "what hub does"
ui/
  cli/       # hub binary — bootstraps config, wires deps, calls workflows
  tui/       # hub-tui binary
scripts/     # dev/ops scripts; not part of the binary
docs/        # architecture, decisions, playbooks
```

`daemon/` sits beside `ui/` rather than inside it because `ui/` means user
interface and the daemon has no user. Both are entry points: they bootstrap
config, wire deps, and call workflows. Decision 020 settles that the daemon
is its own binary; the directory is the only part this repo chose.

Import direction (never import rightward's left neighbor):

```
ui/     → config/               → domain/
daemon/ → workflows/ → clients/ → domain/
                     → store/   → domain/
```

`config/` is a direct dependency of `ui/cli`, `ui/tui` and `daemon/`. Config
values are passed as function arguments into workflows and clients — those
crates do not depend on `config/` directly.

## Stack

| Concern        | Choice                                |
| -------------- | ------------------------------------- |
| Language       | Rust                                  |
| Async runtime  | tokio                                 |
| CLI            | clap (derive)                         |
| TUI            | ratatui                               |
| HTTP clients   | reqwest                               |
| SQLite         | rusqlite (bundled) or sqlx            |
| Serialization  | serde                                 |
| Secrets        | 1Password CLI (`secrecy` + `op read`) |
| Error handling | anyhow                                |

### Rust Conventions

See `~/.claude/references/rust.md` and `~/.claude/references/type-design.md`.

Hard rules for agents:

- **Error handling**: `anyhow` only. No `thiserror`. `?` everywhere. `.context("msg")` for human-readable chains.
- **Owned types**: structs hold `String`/`Vec<T>`. Functions that only read take `&str`/`&[T]`. Return owned values, not references.
- **No lifetime annotations**: if you're writing `'a`, stop and restructure. Return owned types instead.
- **Clone freely**: don't fight the borrow checker. Clone across `.await` points. Optimize only if profiling shows it matters.
- **Async**: `#[tokio::main]`, `features = ["full"]`. Use `tokio::join!` for parallel work. Use `tokio::fs`/`tokio::time` not std equivalents inside async.
- **Secrets**: wrapped in `Secret<String>` (secrecy crate) throughout `Config`, `LokiEnv`, and `StatusParams`. Sourced from `hub.toml`'s `[credentials]` table; `op://` references are resolved at startup via `op read`. `.expose_secret()` is called only at client call sites.
- **CLI**: `clap` with derive macros. Annotate structs; don't use the builder API.
- **Newtypes over primitives**: IDs, status values, and domain-meaningful strings are
  wrapped in newtypes defined in `domain/`, not passed as bare `u64` or `String`.
  `RepoSlug` (already in `domain/`) is the model: one construction path, validation
  baked in, impossible to substitute for an unrelated string. New domain concepts
  follow the same pattern — the type is proof of validity, not a comment.

## Development

```bash
just check   # fmt + lint (autofixes where possible)
just test    # run all tests
just build   # build all crates
just cli     # run the CLI
just tui     # run the TUI
just daemon  # run one refresh with nobody present, then exit
```

## Verifying TUI changes

TUI verification has two tiers depending on what changed.

**Tier 1 — snapshot tests (rendering and layout changes)**

Full-screen `insta` snapshots cover all major screen states (see
`ui/tui/README.md` for the full list and conventions). If a rendering
change causes a visual regression, a snapshot diff will show exactly what
changed. Run `just test` and review any failures.

If the diff is intentional, accept it:

```bash
just test-update
```

When adding a new screen state or item type, add a snapshot for it —
don't rely on the existing snapshots to catch regressions in new code
paths.

**Tier 2 — tmux E2E (interaction and behavior changes)**

For changes that affect keybindings, navigation between screens,
subprocess launching, tmux integration, store schema, cache format, or
domain types the TUI deserializes on startup, snapshots are not sufficient.
Run the TUI live in tmux and drive the interaction.

**Choosing the signal you test against.** Signals come from live APIs, so you
cannot pick the text of a real one. `just qa-seed` writes a synthetic signal
into the status cache, backing the database up first and refusing to run twice
so a forgotten restore cannot cost you the real cache. `just qa-restore` puts it
back. Always restore, including when a run fails partway.

```bash
just qa-seed loki                      # or gcp, media-blocked, ci
just qa-seed media-blocked --text 'whatever you need the agent to see'
just qa-restore
```

The default text is an injection probe: a forged `</untrusted-input>` closing
tag, an instruction, and a `$(touch /tmp/hub-qa-probe)` that is obvious if it
ever runs.

**The loop.**

```bash
cargo build -p hub-tui --features private        # send-keys races a cargo build
just qa-seed loki
tmux new-session -d -s qa -x 200 -y 50 -c "$PWD"
tmux send-keys -t qa:1 "./target/debug/hub-tui" Enter
sleep 12                                          # wait for the first render
tmux capture-pane -t qa:1 -p | head -5
tmux send-keys -t qa:1 "i"
sleep 12
tmux list-panes -s -t qa -F '#{window_name} :: #{pane_start_command}'
```

Then clean up every time: `tmux kill-session -t qa`, `just qa-restore`, remove
any `investigation-*` worktrees left under `~/.hub/repos/<project>/`, and delete
`/tmp/hub-supporting-data-*.json`.

**Gotchas that cost time**

- **Windows are 1-indexed here.** `qa:0` fails with `can't find window: 0`.
- **`capture-pane` shows the shell until the TUI enters the alternate screen.**
  An empty-looking capture usually means it has not rendered yet, not that it
  crashed. Poll `tmux display-message -p -t qa:1 '#{alternate_on}'` for `1`.
- **`op whoami` is not the credentials check.** It reports the CLI session,
  which says `account is not signed in` on a working machine, because the
  1Password desktop app's CLI integration authorises each `op read` on its own
  instead of creating a session. A signed-out `op whoami` is never a reason to
  skip running hub. See
  [the credentials question](docs/questions/how-should-an-unattended-daemon-obtain-credentials.md).
- **Running hub interrupts the user, repeatedly.** `op read` raises a prompt on
  the 1Password desktop app that a human answers with a fingerprint, and
  `Config::load` resolves every `op://` reference in `hub.toml` before the first
  fetch, so one run costs several prompts rather than one. Say before triggering
  a run that loads config, and batch those runs instead of scattering them.
- **A hang with nothing on screen means the desktop app is not running.** In
  that state `op read` blocks rather than prompting, so the process waits with
  no output. That is the symptom to diagnose on, not anything `op whoami` says.
- **You cannot `echo` inside a launched investigation window** — it is running
  Claude Code. Read the prompts off the process instead:
  `ps -p $(tmux display-message -p -t qa:2.0 '#{pane_pid}') -wwE -o command=`.
- **`#{pane_start_command}` is the exact command tmux was handed**, which is what
  to assert against when the change affects how an investigation is launched.

**tmux send-keys pitfalls**

- Use named keys for special keys: `"Enter"`, `"Escape"`, `"Backspace"`,
  `"Up"`, `"Down"`, `"Tab"`. An empty string `""` sends **nothing** — it is
  not a shorthand for Enter.
- When testing the filter query flow: commit the query with `"Enter"` before
  pressing `"/"` again. If the query is not committed the TUI stays in query
  mode and the second `"/"` is treated as `AppendQuery('/')`, not `StartQuery`.

If E2E validation cannot be run, explicitly state why and what weaker
validation was run instead. Before concluding it cannot be run, verify
by inspection: read the config, check the relevant directories, confirm
what is actually available. Never assume a prerequisite is missing.

## Docs by Area

### Conventions and architecture

The organizing rule: **`architecture/` describes what is built; `vision.md`
and the `decisions/` ADRs describe where we are going.** One concept, one home.

**An accepted ADR is not a built system.** An ADR whose subject has not been
built carries `_Status: accepted; not yet implemented._` under its title, naming
the file or symbol that proves it. So:

```bash
rg "not yet implemented" docs/decisions/
```

is the list of designs that are settled and pending. Read it before writing code
against anything an ADR describes, and delete an ADR's line in the change that
builds it.

| Doc                                      | Covers                                                                                                            |
| ---------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `docs/vision.md`                         | **Read this first** — the why; the trust-building flywheel; what we are and are not building                      |
| `docs/architecture/worktrees.md`         | PR investigation worktrees — read before touching `fetch.rs`                                                      |
| `docs/architecture/secrets.md`           | 1Password → op read → Secret<String> model                                                                        |
| `docs/architecture/private-workflows.md` | Two-repo model for private workflows                                                                              |
| `docs/decisions/`                        | ADRs (rationale). Unbuilt ones say so under the title — see the note above          |
| `docs/invariants/`                       | What must always hold, and the check that holds it up. No exceptions, unlike a standard |
| `clients/README.md`                      | reqwest pattern for HTTP clients                                                                                  |
| `store/README.md`                        | rusqlite pattern, db path, Connection threading notes                                                             |
| `ui/cli/README.md`                       | clap derive API for CLI commands                                                                                  |
| `ui/tui/README.md`                       | TUI architecture, cache/schema version, keybindings                                                               |

### Playbooks

| Doc                                                     | Covers                                      |
| ------------------------------------------------------- | ------------------------------------------- |
| `docs/playbooks/add-a-workflow.md`                      | Adding a new workflow end to end            |
| `docs/playbooks/add-a-project.md`                       | Adding a project to a device config         |
| `docs/playbooks/add-a-private-workflow.md`              | Adding a workflow to hub-private            |
| `docs/playbooks/set-up-private-workflows-repository.md` | First-time or recovery setup of hub-private |

## File relationships

- `AGENTS.md` and `CLAUDE.md` are symlinked
- `.agents/skills/` and `.claude/skills/` are symlinked

## File and module boundaries

Every `lib.rs` and `mod.rs` is navigation only — module declarations and re-exports, nothing else. Every concept lives in a file named after it; `ls src/` should read as a glossary. Files over ~400 lines are a signal that a boundary exists and wants a name.

## Directory notes

`.agents/` is the agent harness tracking directory — it holds skill files and
session state for agent runs. It is not a Rust crate. The `agents/` Rust crate
for background automation is described in Decision 005 but has not been built yet.
