# A refresh that reached no source never replaces the cache

## The invariant

Neither cache writer, `hub-daemon` or `hub-tui`, replaces the `status_cache` row with a refresh that
came back with no signals while a source failed. Such a refresh leaves the previous row exactly as
it was. Both writers decide with one rule, `classify` in `workflows/src/freshness.rs`.

## Why it must hold

`workflows::status::run` returns `Ok` whether or not anything answered. A failing source is
collected into `StatusReport::errors` and the remaining sources still contribute items, so a total
outage produces a perfectly well-formed report with an empty `items` list.

Writing that report is data loss. The cache is a single row and every write replaces it, so the
twelve pull requests it held a minute ago are gone, and nothing can recover them until a later
refresh succeeds. The TUI reads that row, and the notification work in
[#327](https://github.com/ooloth/hub/issues/327) will read it too, so a wiped row is a queue that
silently reports nothing to notify about.

The daemon is what makes this sharp. A human watching the TUI sees the error banner and knows to
distrust the empty list. Nobody is watching the daemon, so an empty row written at 3am is
indistinguishable from a quiet morning.

## What it forbids

- Writing a report whose `items` is empty when `errors` is not. This is the whole invariant, and it
  is the case that looks harmless because the report is valid and the write succeeds.
- Skipping the write when `items` is empty and `errors` is also empty. That is the opposite error
  and equally wrong: a queue that genuinely drained is a real answer, and refusing to record it
  leaves the cache asserting stale work that no longer exists. The condition is a conjunction for
  this reason.
- Calling `store::status_cache::upsert` from anywhere in `daemon/`, or
  `store::daemon_health::record` from anywhere in `daemon/` other than `daemon/src/cache.rs`, which
  is the only place that has checked.
- Calling `store::status_cache::upsert` from anywhere in `ui/tui/` other than the
  `Effect::WriteCache` handler in `ui/tui/src/main.rs`, which receives only a checked payload.

What it permits: writing a partial refresh, where some sources failed and others answered. The
report carries the failed source names, `RefreshState::Partial` renders them, and the items that
did arrive are real.

## How it is enforced

**The construction path is enforced by the compiler, totally.** `workflows::freshness::FreshStatus`
wraps the report in a private field and `classify` is its only constructor. A payload reaches the
daemon's `cache::record` and the TUI's `Effect::WriteCache` only as a `FreshPayload`, whose only
constructor is `FreshStatus::payload`. There is no way to hand a total-outage report to either
writer, and no check is needed for that path.

**The branch is covered by a test, not the compiler.** `settle` in `daemon/src/main.rs` turns a
`NothingRefreshed` into a pass with no payload, and nothing stops a future edit from passing one
anyway. That is what `a_refresh_that_reached_no_source_leaves_the_previous_row_untouched` in
`daemon/src/cache.rs` is for: it records a pass that wrote a payload, records one where every
source failed after it, and asserts the payload, schema version and `refreshed_at` are all
unchanged while the health record moved on.

In the TUI, `apply_refresh` in `ui/tui/src/state/update/refresh.rs` turns a `NothingRefreshed` into
no effect at all. `a_refresh_with_no_signals_and_a_failure_does_not_write_the_cache` in
`ui/tui/src/state/update/mod.rs` covers that branch, and the tests beside it check that the screen
keeps the earlier signals and their time.

What that misses: a new file under `daemon/src/` or `ui/tui/src/` calling
`store::status_cache::upsert` on its own bypasses both mechanisms. Nothing greps for that today.
The `domain_reads_no_ambient_state` test in `scripts/src/repo_state/domain_purity.rs` is the model
for the tripwire that would close it.

[Decision 022](../decisions/022-tui-reads-cache-never-fetches.md) removes the TUI's fetch, and with
it the TUI's write. The daemon is then the only writer this invariant covers.
