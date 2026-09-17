---
number: 021
status: accepted
date: 2026-09-16
---

# 021 — The daemon owns signal refresh and is the only cache writer

_Status: accepted; not yet implemented. `ui/tui/src/main.rs` still drives the
refresh interval and calls `store::status_cache::upsert`._

## Forced by

[Decision 020](020-hub-runs-an-unattended-surface.md) puts a process on the machine that has to
know the current state of the PR queue in order to notify about it.

[Decision 008](008-tui-owns-refresh-loop.md) placed the refresh loop inside the TUI process and
records the consequence at line 45: "There is no background refresh when the TUI is not open."
Both cannot hold. Something has to fetch while the TUI is closed, and 008 says nothing does.

## Decision

The daemon polls on an interval and writes the SQLite cache. It is the only writer.

Refresh ownership moves out of the TUI process entirely rather than being duplicated. With one
writer, a notification and the screen are always computed from the same rows, so a row does not
have to record which process wrote it and a reader never has to reconcile two versions of now.

This moves nothing else. `hub` the CLI still fetches live on each invocation and does not touch the
cache, which is what [Decision 008](008-tui-owns-refresh-loop.md) established and this record has
no reason to disturb.

## Rejected

- **Dual fetch: the TUI keeps its interval and the daemon runs its own** — because two writers to
  one cache means the data on screen and the data behind a notification are the products of
  different passes, and nothing in a row says which. A user comparing a banner against the list
  cannot tell disagreement from lag. Reverses if rows gain per-writer provenance.
- **launchd invoking a one-shot process once per interval, instead of a long-lived one** — because
  a process that exits after each pass is not there to be asked anything between passes. The TUI
  has two things to ask it, both in #327's plan of record: whether the daemon is alive, and to
  refresh now. The plan answers them with a Unix socket at `$HUB_HOME/hub.sock`, and a process that
  exists for a few seconds an hour has nothing to bind it to. Reverses if the TUI stops needing to
  talk to the daemon at all.
- **Not yet: leave 008 in place and have the daemon notify from whatever the TUI last wrote** —
  because the TUI may not have run for days, so the notification would describe a queue that has
  moved on. It is the cheapest option and it fails at exactly the moment it is needed. Reverses if
  the TUI runs continuously enough that its cache is never stale, which is the assumption 008 was
  built on.

## Risk

A daemon that dies takes freshness with it, and after
[Decision 022](022-tui-reads-cache-never-fetches.md) hub has no other fetch path. Liveness,
crash recovery and single-instance guarding stop being polish and become part of what has to work
before this is usable. The failure is quiet by nature: a dead daemon produces no notifications, and
no notifications is also what a quiet week looks like.

## Revisit when

Keeping the daemon alive costs more than it saves, observable as recurring manual restarts.

## Also update

- [x] questions/README.md — `should-refresh-run-on-a-per-category-schedule.md` stays open and now
      attaches to the daemon rather than the TUI.
- [x] vision.md — the daemon joins the surfaces section as the owner of refresh.
