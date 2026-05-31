# 011 — TUI extends to session and task tracking; single process continues

## Context

[Decision 008](008-tui-owns-refresh-loop.md) placed the refresh loop in
the TUI process. Hub's evolving direction toward being a personal IDE
adds two responsibilities that need a home: tracking the lifecycle of
agent sessions, and managing tasks the user accumulates across projects
with cross-references to agent reports, PR URLs, and related signals.

The natural fork was between:

- **A — A separate long-running backend service** owning these
  responsibilities, with the TUI as a thin client.
- **B — Extending the single-process model** from decision 008 so the
  TUI owns these responsibilities directly.

A backend service (a daemon) was considered. Each load-bearing
capability that initially motivated it turned out to be solvable
without one:

- Agent processes live in tmux, not the TUI. The TUI is an observer,
  not the agent's parent. Closing the TUI does not kill the agent.
- Session records persist in SQLite. The TUI writes them on launch and
  reads them on restart.
- Transcripts persist on disk, written by claude code itself. The TUI
  reads them as needed.
- Jump and resume work via tmux and claude commands, with no
  coordinator required.

What a daemon would still uniquely give — always-fresh signals while no
UI is open, push notifications during those gaps — works against hub's
[human-initiated philosophy from decision 009](009-no-scheduled-runs.md).
Hub is opened intentionally when the user wants to work. A "polls and
is fresh within seconds of opening" experience is indistinguishable in
practice from "was polling the whole time" for a tool used this way.

## Decision

Option B. The TUI continues to own the refresh loop and additionally
owns session tracking and task management within the same single
process.

Agent processes themselves run in tmux windows; the TUI does not
parent them. The TUI's role is observer, recorder, and coordinator
across sessions.

Multi-instance coordination is handled via single-instance behavior — a
second launch attaches to the existing TUI rather than starting fresh.
The shape and mechanism are deferred to implementation.

## Consequences

- Decision 008 still holds and is extended in scope by this decision.
- A separate backend service is not built. If hooks ever require
  inbound HTTP, or always-on freshness becomes a real need, the
  upgrade path is additive — promote background tasks to a separate
  binary or add an Axum surface at that point.
- Session records and task records persist in SQLite. The `store/`
  crate extends accordingly.
- Transcripts on disk (written by claude code) are the source of truth
  for session output. The TUI reads them as needed.
- [Decision 007](007-tui-over-web-app.md) is unchanged. The TUI is the
  primary UI.
- [Decision 009](009-no-scheduled-runs.md) is reinforced. Hub is
  human-initiated, not always-watching.
