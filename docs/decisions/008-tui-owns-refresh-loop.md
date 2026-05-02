# 008 — TUI owns the refresh loop; no separate daemon

## Context

Hub's status data comes from live network calls (GitHub, Linear, private
integrations). The CLI blocks on these calls each invocation — tolerable
for a command run a few times a day, but wrong for the TUI, which must
render immediately on launch and stay current without user action.

The natural options were:

**A — Separate `hub daemon` command.** A long-lived background process
that fetches on a schedule and writes to SQLite. Both CLI and TUI read
from the cache. Daemon must be running for either to see fresh data.

**B — TUI owns the refresh loop.** The TUI process starts its own tokio
interval task, reads from SQLite on launch (whatever is cached), and
writes back after each refresh. No separate process. The CLI continues
doing live fetches, but can also read from the same SQLite cache as a
convenience when the TUI has been running recently.

## Decision

Option B. The TUI owns the refresh loop.

A separate daemon adds surface area without a proportionate benefit at
this stage. The CLI doesn't need sub-second startup — a 2-3 second live
fetch is acceptable. The TUI does need instant startup, and it needs
auto-refresh, but both are internal concerns: the TUI can seed itself
from SQLite and run its own interval without any external process.

The daemon pattern raises questions that aren't worth answering yet: how
do you know if it's running, how do you start it on boot, what happens
when it crashes. Option B defers all of that. If hub eventually runs on
a server or needs refresh when no TUI is open (e.g. to push alerts), a
daemon is the right answer at that point.

## Consequences

- The TUI is self-contained: launch it, it shows cached data immediately,
  refreshes on its own schedule, no external process required.
- `hub status` (CLI) reads from SQLite if the cache is fresh (≤ 30 min),
  falls back to a live fetch with a notice if stale or absent. This means
  CLI reads benefit from a recently running TUI at no extra cost.
- There is no background refresh when the TUI is not open. If the TUI
  hasn't been running, the CLI live-fetches on the first invocation and
  the result warms the cache for subsequent calls in that session.
- If a future use case requires refresh without an open TUI (scheduled
  alerts, headless server operation), revisit this decision and introduce
  a daemon then. The SQLite schema is already the right shape for it.
