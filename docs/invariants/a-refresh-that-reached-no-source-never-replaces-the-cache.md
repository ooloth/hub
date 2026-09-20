# A refresh that reached no source never replaces the cache

## The invariant

`hub-daemon` replaces the `status_cache` row only with a refresh that at least one source answered.
A refresh where every source failed leaves the previous row exactly as it was.

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
- Calling `store::status_cache::upsert` from anywhere in `daemon/` other than `daemon/src/cache.rs`,
  which is the only place that has checked.

What it permits: writing a partial refresh, where some sources failed and others answered. The
report carries the failed source names, `RefreshState::Partial` renders them, and the items that
did arrive are real.

## How it is enforced

**The construction path is enforced by the compiler, totally.** `daemon::freshness::FreshStatus`
wraps the report in a private field, `classify` is its only constructor, and `cache::write` accepts
nothing else. There is no way to hand a total-outage report to the writer, and no check is needed
for that path.

**The branch is covered by a test, not the compiler.** `cache::apply` chooses whether to write, and
nothing stops a future edit from calling `upsert` directly in the `NothingRefreshed` arm. That is
what `a_refresh_that_reached_no_source_leaves_the_previous_row_untouched` in `daemon/src/cache.rs`
is for: it seeds a row, applies a failed refresh over it, and asserts the payload, schema version
and `refreshed_at` are all unchanged.

What that misses: a new file under `daemon/src/` calling `store::status_cache::upsert` on its own
bypasses both mechanisms. Nothing greps for that today. `scripts/check-domain-is-pure.sh` is the
model for the tripwire that would close it.

**The TUI does not uphold this and is outside its scope.** `ui/tui/src/main.rs` is still a second
cache writer and writes every report unconditionally, including a total outage.
[Decision 021](../decisions/021-daemon-owns-signal-refresh.md) makes the daemon the only writer, and
Phase 5 of #327 removes the TUI's write. Until that lands, this invariant holds for the daemon and
not for the cache as a whole.
