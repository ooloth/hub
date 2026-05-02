# 004 — Where observations are tracked

## Context

Hub observes signals from external systems (PRs, errors, download
health, code quality, etc.) and needs a place to persist what it finds
— both for fast local display and for durable tracking of things that
warrant human attention.

## Decision

Two persistence layers, for different purposes:

**SQLite (`store/`)** — local cache of fetched data. Refreshed on each
sync. Holds current state (open PRs, recent errors, counts) and
historical data (trends over time). Also holds TUI state (seen,
snoozed, dismissed). Ephemeral by nature: when the source resolves,
the cached item goes away.

**Issue trackers** — durable action items hub creates when it infers
something warrants human attention beyond what the source system
already tracks. Not "here's what exists" but "hub decided this needs
to be acted on." The issue lives where it belongs:

| Observation type | Where the issue is filed |
|---|---|
| Code quality, bugs, tech debt | Source repo's GitHub Issues |
| Work operational concerns | Linear (work context) |
| Personal/cross-cutting | hub-private repo's GitHub Issues |

Hub files the issue and forgets. On the next sync it reads it back via
search (`is:open is:issue assignee:@me` across all repos). The issue
tracker is the source of truth; SQLite is a read cache of search
results; hub is the triage layer on top.

## Status cache schema

The SQLite status cache (`store/`) uses a single-row table with four
columns: `id`, `schema_version`, `refreshed_at`, and `payload` (a JSON
blob of the full `StatusReport`). One row is upserted on each refresh;
there is no history.

`schema_version` is an integer constant in the codebase, bumped whenever
`StatusReport`'s shape changes in a backward-incompatible way. Readers
check this column before deserializing and discard the row if the version
doesn't match, triggering a live fetch.

Stale threshold is 30 minutes. The CLI prints `last updated Xm ago` when
reading from cache; if the cache is older than 30 minutes or absent, it
prints a notice, fetches live, and upserts the result. The TUI starts
from whatever is cached (instant render), then refreshes in the
background if the cache is older than 30 minutes. See
[008](008-tui-owns-refresh-loop.md) for why the TUI owns this loop.

## Consequences

- Hub never needs to know it filed a specific issue — it just searches
  for open issues assigned to or mentioning the user and surfaces them.
- Issues filed in source repos are visible to collaborators in context,
  not hidden inside hub.
- The same GitHub search query that surfaces manually-created issues
  also surfaces hub-created ones — no special handling needed.
- Linear and Notion both have cross-team/cross-database query APIs that
  work the same way for work and personal contexts respectively.
- SQLite is not the source of truth for action items — it is a
  performance cache. If the local db is deleted, a sync restores
  current state from the source systems.
