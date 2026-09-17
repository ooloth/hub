---
number: 022
status: accepted
date: 2026-09-16
---

# 022 — The TUI reads the cache and never fetches

## Forced by

[Decision 021](021-daemon-owns-signal-refresh.md) makes the daemon the only cache writer, which
removes the write half of the loop [Decision 008](008-tui-owns-refresh-loop.md) gave the TUI at
lines 39 to 44.

That leaves one thing undecided: whether the TUI may still fetch for itself when the daemon is
absent. 021 does not answer it. A read-only fetch that refreshes the screen without touching the
cache would violate nothing 021 says.

## Decision

It may not. The TUI renders the cache and nothing else.

When the daemon is not running, the TUI shows the data it has, says how old it is, and says the
daemon is down. Stale and labelled beats current and misleading, because the daemon being down is
itself the most important thing on the screen: it means notifications stopped.

## Rejected

- **Fall back to fetching when the daemon is unreachable** — because a TUI that quietly fetches
  for itself makes a dead daemon indistinguishable from a live one. The screen looks right and the
  only symptom is notifications that never arrive, which is the failure
  [#327](https://github.com/ooloth/hub/issues/327) exists to fix. Reverses if notifications stop
  being the daemon's purpose.
- **A `--standalone` flag that re-enables fetching for development** — because it is the bullet
  above with a switch on it, and a switch does not change what the code path does when it is on.
  The motivation does not survive either: per #327's plan of record a dev TUI fails by colliding
  with the installed instance's socket, database and logs, not for want of a fetch path, and
  resolving all three from `HUB_HOME` is what fixes it. Reverses if a case appears where the TUI
  has to run somewhere no daemon can.

## Risk

Every TUI launch now depends on a process the user has to keep running, and the first launch after
a fresh install has an empty cache and no daemon. An empty screen is the default failure, and it
has two causes that look identical: nothing has ever been fetched, or the daemon has stopped. The
empty state has to name which one and say what to run. That is a requirement this decision creates,
not a nicety.

## Revisit when

The TUI needs to run where the daemon cannot, such as over a remote shell onto a machine that is
not the one polling.

## Also update

- [x] questions/README.md — no open question closes into this record.
- [x] vision.md — the TUI's entry in the surfaces section no longer promises auto-refresh.
