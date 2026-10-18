# The daemon's health and the signals are only read together

## The invariant

Every reader of the profile's database gets the cached signals and the daemon's health record from
one call, `store::snapshot::read`, which reads both inside one read transaction.

## Why it must hold

The two live in separate rows ([Decision 026](../decisions/026-daemon-health-is-one-row-in-its-own-table.md)).
The daemon replaces both in one write transaction, but a reader that reads them in separate
transactions can land between two passes: it sees one pass's signals beside the next pass's
health, or the reverse. The TUI would then say refreshes are current beside signals from before
they stopped, or warn about a stopped daemon beside signals it has just written. Under SQLite's
write-ahead log, every statement inside one read transaction sees the same snapshot, so reading
both there rules that out.

## What it forbids

- Reading `status_cache` or `daemon_health` from outside the `store` crate by any route other than
  `store::snapshot::read`.
- Adding a public function to `store` that reads either table on its own, even one that looks
  read-only and harmless, such as a freshness check on the cache row.
- Reading the two through `snapshot::read`'s pieces in two separate transactions inside `store`.

## How it is enforced

**Outside `store`, by the compiler, totally.** `status_cache::read` and
`daemon_health::last_record` are `pub(crate)`, so nothing outside the crate can call them, and
`snapshot::read` is the only public read of either table. The test-support
`daemon_health::recorded` is compiled only for tests.

**Inside `store`, by review.** Nothing stops a new function in `store` from reading one table
alone and being made public. `only_the_store_names_the_daemon_health_table` in
`scripts/src/repo_state/daemon_health_writer.rs` keeps the health table's SQL inside
`store/src/daemon_health.rs`, which narrows where such a function could appear, but it does not
check visibility.
