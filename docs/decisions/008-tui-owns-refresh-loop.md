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
doing live fetches on every invocation; it does not read from the cache.

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
- `hub status` (CLI) always does a live fetch and exits. The cache is a
  TUI-only concern. CLI is a one-shot command run infrequently; a 2-3
  second live fetch is acceptable and avoids any cache-staleness
  complexity in the CLI path.
- There is no background refresh when the TUI is not open.
