# hub

## Private constraints

If `../hub-private/CLAUDE.md` exists, read it before writing or editing any files — it lists terms that must never appear in committed hub files.

Every change must build on devices with and without each hub-private module, not only on the
device it was written on. `just lint` checks every configuration the checkout can build. See
[the invariant](docs/invariants/hub-builds-with-and-without-each-private-module.md).

## What This Is

Hub is a personal command center that aggregates signals from multiple sources — GitHub PRs, CI status, Loki alerts, Linear issues, and more via the `private` feature — into a single urgency-ranked terminal view, and delegates action on those signals to agents via filesystem-based investigation sessions.

Its three binaries serve three distinct audiences:

- **`hub-tui`** (Ratatui dashboard) — the **human-facing surface**. Read signals, launch investigation sessions, watch session progress, review results.
- **`hub`** (CLI) — the **agent's toolkit** (stub). The task subcommand (`hub task *`) was removed with the task model (ADR 019). Future agent-facing subcommands will be added here as the filesystem session model is built out.
- **`hub-daemon`** — the **unattended surface** (ADR 020). Refreshes the signal cache with nobody present. It refreshes every 15 minutes (`--interval`), one daemon per profile, and `--once` runs a single pass. Each pass is recorded in a health record in the profile's database. `just daemon-start` runs the installed binary under launchd at every login. Credentials with nobody present, notifications and showing health in the TUI are the rest of the [#327](https://github.com/ooloth/hub/issues/327) milestone.

The core value is cross-domain triage plus agent delegation: signals from different systems are ranked together in one list, and any signal can be investigated by pressing `i` to launch a Claude Code session with injected context. That session opens in its own tmux window, named after the signal by `domain::InvestigationWindow`, so several investigations run side by side and each stays reachable from tmux's window list.

See [README.md](README.md) for the full feature list and value proposition.

## Current milestone

The current milestone is the open GitHub milestone with at least one closed issue. If two
qualify, the `M1:` / `M2:` prefix in their titles orders them. The `next` skill finds it and ranks
it against everything else in the tracker.

Each phase issue's parent is the milestone's epic. Before starting anything in the milestone, read
the epic's plan of record:

```bash
gh issue view <phase> --json parent       # finds the epic
gh issue view <epic> --comments
```

The plan lives in a **comment**. `gh issue view <epic>` on its own prints the body and stops, and
the body can predate the plan, so reading it alone can give you a superseded design. The comment
carries the settled architecture, the measured spike findings, and the full phase sequence.

**The title numbering is the work order.** Issues are titled `Phase N.M — ...`; take them in that
order. Hard dependencies are recorded separately as GitHub `blockedBy` relationships, which
`gh issue view N --json blockedBy` prints. Everything else the numbering implies is sequence, not
blocking.

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
prompts/     # investigation prompts loaded by the TUI
scripts/     # the `scripts` crate: dev/ops tooling, ships in no binary (Decision 025)
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

See `~/.agents/standards/rust.md` and `~/.agents/standards/type-design.md`.

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
just daemon  # refresh every 15 minutes with nobody present (--once for one pass)
just daemon-start  # run the installed daemon under launchd at every login (daemon-stop, daemon-logs)
```

Every `just` recipe that runs hub from source uses the `dev` profile (`~/.hub/dev/hub.db`).
`HUB_PROFILE=default just …` reaches the installed hub's state. The `daemon-*` recipes manage the
installed daemon, which always uses `default`.

A Claude Code hook guards `gh` posts against hub-private's banned terms. It runs the built
`scripts` binary, so it needs `just build` once per checkout (see `scripts/README.md`).

## Verifying TUI changes

**Every change that alters what the TUI shows or how it behaves is driven live
in tmux before it is reported as working.** That includes rendering and layout
changes, not only interaction ones. The point of the live run is to find
behaviour the tests did not predict, so it happens even when every test and
every snapshot is green — especially then.

A live run resolves credentials through `op read`, which raises 1Password
prompts the user answers with a fingerprint. That is normal and expected, and
it is never a reason to skip a run, defer one, or ask permission first.

**Snapshots — necessary, never sufficient**

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

What a snapshot cannot show is anything outside the buffer this code
produces: the real terminal's width and wrapping, real signal text, the
startup path, timing, and how the screen behaves once keys arrive. Those need
the live run.

**Driving it live**

Run the TUI in tmux, drive the interaction, and read the screen back. Look
past the thing you changed: check the rows either side of it, resize the
window, move the selection, open and close a detail pane. Report what you
observed, not that it rendered.

**Choosing the signal you test against.** Signals come from live APIs, so a live
run covers whichever signals exist at the time. When the change concerns a kind
of signal that has none right now, the report says that path went unobserved
live.

**Seeing how hub behaves when sources fail.** `just sources-unreachable <recipe>`
runs any recipe with every source refused, as a network failure would refuse it,
while 1Password still answers: `just sources-unreachable daemon --once`, or
`just sources-unreachable tui`. It points the proxy variables at a closed local
port and exempts 1Password's domains, so it does not cut 1Password, DNS, or
anything that ignores proxy variables, such as `git` over SSH. The GitHub sources
and the private workflows have been seen to fail through it; Linear, Loki and GCP
have not been tried. A TUI refresh through it currently empties the cache
([#343](https://github.com/ooloth/hub/issues/343)), so run `just daemon --once`
afterwards to refill it.

**The loop.**

```bash
just build                                        # send-keys races a cargo build
tmux new-session -d -s qa -x 200 -y 50 -c "$PWD"
tmux send-keys -t qa:1 "just tui" Enter
sleep 12                                          # wait for the first render
tmux capture-pane -t qa:1 -p
tmux send-keys -t qa:1 "i"
sleep 12
tmux list-panes -s -t qa -F '#{window_name} :: #{pane_start_command}'
```

Then clean up every time: `tmux kill-session -t qa`, remove any
`investigation-*` worktrees left under `~/.hub/repos/<project>/`, and delete
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
- **`op read` raises 1Password prompts, and that is normal.** `Config::load`
  resolves every `op://` reference in `hub.toml` before the first fetch, so one
  run raises several prompts, each answered with a fingerprint. Expect them.
  They are never a reason to skip a live run or to ask before starting one.
- **`couldn't connect to the 1Password desktop app` means the app is not
  running.** `op read` fails within a second with that message, before any
  fetch starts. Start the app and run again. The first run after starting it
  can fail the same way while the app finishes launching (observed 2026-10-02).
- **A hang with nothing on screen is a different state.** `op read` is waiting
  on something nobody has answered. A 2026-09-21 measurement attributed this to
  the app not running, which a 2026-10-02 run contradicts. The cause is open in
  `docs/questions/how-should-an-unattended-daemon-obtain-credentials.md`.
  Diagnose on this symptom, not on anything `op whoami` says.
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
| `docs/questions/`                        | Open design questions, one per file                                                                               |
| `docs/decisions/`                        | ADRs (rationale). Unbuilt ones say so under the title — see the note above          |
| `docs/invariants/`                       | What must always hold, and the check that holds it up. No exceptions, unlike a standard |
| `clients/README.md`                      | reqwest pattern for HTTP clients                                                                                  |
| `store/README.md`                        | rusqlite pattern, db path, Connection threading notes                                                             |
| `ui/cli/README.md`                       | clap derive API for CLI commands                                                                                  |
| `ui/tui/README.md`                       | TUI architecture, cache/schema version, keybindings                                                               |
| `daemon/README.md`                       | The unattended refresh surface: what lives in `daemon/` and its rules                                             |
| `scripts/README.md`                      | The `scripts` crate: repository checks, Claude Code hooks, local setup                                            |

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
