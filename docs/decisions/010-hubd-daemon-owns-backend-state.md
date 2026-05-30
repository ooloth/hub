# 010 — `hubd` daemon owns backend state; TUI becomes a thin client

## Context

Decision 008 placed the refresh loop in the TUI process. The reasoning was
sound for the scope it considered: cached signals polled occasionally, no
long-running operations, no cross-UI state. A daemon would have been
overhead with no proportionate benefit at that stage.

The scope has changed.

Hub's direction is evolving toward being a personal IDE — an outer loop for
an engineering practice, where signals come in, decisions get made, agent
sessions get launched and supervised, and what was learned gets retained
for future runs. The forcing function for this decision is **supervisory
agent sessions that must outlive any single UI process**.

Three concrete capabilities emerged that don't fit in a UI-hosted process:

1. **Multi-session supervision.** An agent run starts at 14:10 and takes
   30 minutes. The user closes the TUI to focus elsewhere and reopens it
   at 14:35. Hub must show "finished 12 minutes ago, PR opened" — not
   "session lost, no record." Fire-and-forget tmux launches (hub today)
   cannot do this.

2. **Per-source background polling at different cadences.** A CI failure
   should appear within 60 seconds; a PR list can refresh every 5 minutes;
   a task list every 15. The current TUI-owned 30-minute all-or-nothing
   refresh coarsens everything to the slowest cadence and only runs while
   the TUI is open.

3. **Persistent state across UI instances and restarts.** Event log,
   trend cache, in-flight agent sessions, workspace registry, artifact
   registry — all need to exist whether or not a UI is currently rendering
   them. Multiple TUI instances should be first-class viewers onto shared
   state, not independent processes racing each other.

The natural options were:

**A — Continue TUI-owns-refresh.** Keep the current model. Long agent
sessions die on TUI close or detach as orphans the TUI cannot recover.
Multi-cadence polling and notify-while-closed are not solvable in this
shape.

**B — Detached subprocess workarounds.** Status files on disk, pidfile-based
"first TUI is leader" patterns, polling SQLite for state changes. Each
approximates a daemon, badly. By the time any of these is hardened, a
daemon exists in all but name.

**C — A `hubd` daemon owning all backend state.** Long-running Rust binary
hosts the scheduler, agent session manager, workspace manager, event log,
trend cache, and editorial layer. UI clients connect over a unix socket
and observe state.

## Decision

Option C. A new `hubd` binary owns all backend state. The TUI becomes a
thin client.

`hubd` is a tokio-based Rust binary that runs in the background, started
via `launchd` on macOS (or a manual `tmux new -d hubd` for development).
It exposes a unix socket at `~/.hub/hubd.sock` for local clients. HTTP+WS
over a local port is available behind a feature flag for future UI
optionality but is off by default.

The TUI process:

- Owns no scheduler, no session state, no agent subprocess.
- Connects to `hubd` on launch, subscribes to state changes, renders.
- Multiple TUI instances are first-class — each is a viewer onto shared
  state.
- Closing the TUI has no effect on running agent sessions.

The `hub` CLI binary drops its status-display role entirely. Its sole
purpose becomes serving as an on-the-fly toolkit for agents — deterministic
command wrappers for git, GitHub, and other real-world interactions, with
project-conditional logic baked in. Agents call `hub` during sessions;
users no longer invoke it to view status. The TUI is the only human-facing
surface.

## Consequences

- **Decision 008 is superseded.** Its reasoning is preserved for the
  historical record; the conclusion no longer holds at the current scope.
- **`hubd` is a new top-level crate.** It hosts the scheduler, agent session
  manager, workspace manager, event log, trend cache, artifact registry,
  and editorial layer. Existing crates (`domain`, `clients`, `workflows`,
  `store`, `config`) become dependencies of `hubd`.
- **The TUI no longer owns the status cache.** Reads come from `hubd` over
  the socket. The single-row status cache in `store/` is replaced by
  richer tables (sessions, events, trends, workspaces, artifacts, links)
  owned by `hubd`. `SCHEMA_VERSION` will need a bump and a one-time
  migration.
- **`workflows/src/status.rs` loses orchestration responsibility.** It
  becomes a domain-only definition (the `StatusReport` and `StatusItem`
  shapes). Orchestration moves into `hubd`.
- **The `hub` CLI loses `hub status`.** The binary stays, repurposed as the
  agent toolkit only. A future decision will document the agent CLI
  surface and conventions.
- **Schedulers run per-source.** Each workflow declares its refresh
  interval (default 5m for most, 60s for errors). The all-or-nothing
  refresh model is gone.
- **Agent sessions become first-class persistent state.** Lifecycle
  (working, paused, done, failed), live transcript, cost accounting,
  progress steps, workspace and artifact links — all persisted, all
  observable from any UI client.
- **OS notifications become possible.** `hubd` can fire notifications on
  session completion, critical signal arrival, or paused-agent input
  requests, independent of any UI being open.
- **Future UI optionality is preserved.** A web UI later costs the
  addition of an HTTP+WS handler on `hubd` — not a re-architecture.
- **Decision 009 still holds.** The daemon polls *data sources*, not
  *agent triggers*. Tier 3 Execute remains human-initiated via TUI keypress.
- **Decision 007 still holds.** The TUI is the primary UI for now. The
  daemon makes a future web UI possible without committing to one.
- **Tier 3 worktree machinery moves into the workspace manager.** What's
  in `workflows/src/implement.rs` and `ui/tui/src/investigations/` for
  launching agents into tmux migrates to `hubd`'s session manager, with
  the TUI dispatching launch requests rather than orchestrating them
  directly.
- **The prompts/skills model is unchanged.** Skills still launch via TUI
  keypress. The daemon manages the resulting session — launch, observe,
  pause, resume, record.
- **Daemon lifecycle is intentionally light.** Crash recovery comes from
  persistent state in SQLite; relaunch is sufficient. No high-availability
  story is in scope. If `hubd` is down, the TUI shows "daemon
  unreachable" rather than attempting to take over its responsibilities.
