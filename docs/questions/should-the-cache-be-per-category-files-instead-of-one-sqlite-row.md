---
opened: 2026-09-16
status: open
resolves_into: decision
---

# Should the cache be per-category files instead of one SQLite row?

## Why it matters

The cache is one row. `ensure_table` in `store/src/status_cache.rs` creates a single table whose
primary key is always written as the literal `1`, holding one `schema_version`, one `refreshed_at`,
and one `payload` that is the whole serialized `StatusReport`. Every read is `SELECT ... FROM
status_cache WHERE id = 1` (`read`). Every write is an upsert on the same key (`upsert`).

That shape assumes one refresh produces the whole world at one instant. Fetching each signal type on
its own cadence breaks the assumption in three places:

- **One timestamp cannot describe a payload of mixed age.** The TUI's "updated Xm ago" reads the
  single `refreshed_at`, and [Decision 022](../decisions/022-tui-reads-cache-never-fetches.md)
  makes saying how old the data is a requirement rather than a nicety. With per-category cadence,
  there is no single honest value to put there.
- **A per-category write has to rewrite everything.** `StatusReport` is `{ items: Vec<StatusItem>,
  errors: Vec<String> }` (`StatusReport` in `workflows/src/status.rs`) and `items` is flat, with no
  category partition. Refreshing one category means reading the blob, filtering that category's
  items out, splicing new ones in, and writing the whole thing back. That is a read-modify-write
  over every other category's data on every refresh of any one of them.
- **`errors` has the same problem.** A per-category refresh cannot clear only its own failure
  entries, because nothing in the list says which category produced which string.

The assumption also breaks without per-category cadence. A refresh where some sources fail still
writes the one row, and the row then holds only the sources that answered. A source that times out
loses its last answer until a later pass reaches it, and every reader of the row, the notifications
in #327 included, sees fewer items than are waiting.

[Decision 021](../decisions/021-daemon-owns-signal-refresh.md) makes the daemon the only writer once
the TUI stops writing (it still does today), which removes the cross-process race but not the
coupling: one writer rewriting the whole blob per category is still every category's data passing
through every category's refresh.

## What would settle it

Two things can make the single-row shape wrong, and each is settled on its own.

The first is per-category cadence. It cannot be answered before the cadence question in
[should-refresh-run-on-a-per-category-schedule.md](should-refresh-run-on-a-per-category-schedule.md)
is answered, and that question waits on milestone #327 shipping and being used, so it cannot be
settled inside that milestone. If refresh stays on one global clock, cadence gives no reason to
change the shape.

The second does not wait on cadence. A partial pass replaces the row without the failed sources'
items (see the 2026-10-05 findings). Two things settle whether that forces a new shape:

1. **Whether a failed source's last answer should stay in the list.** Keeping it shows items that
   may be out of date. Dropping it hides items that may still be waiting. That is a product
   question, and if the answer is to drop them, the current shape already does that.
2. **How often a pass is partial.** The daemon logs `outcome=partial` and the failed sources on
   every pass, so a week of its log measures this. A partial pass once a month is a different claim
   from one every few passes.

If either reason calls for a new shape, two observations settle which one:

1. **The size and frequency of a whole-blob rewrite at realistic signal counts.** Serialize a
   populated `StatusReport` and measure it, then measure the upsert against the daemon's intended
   per-category interval. A payload of tens of kilobytes rewritten hourly is not worth a new shape.
   The same payload rewritten by six categories on independent minute-scale timers is a different
   claim, and only the measurement distinguishes them.
2. **Whether a reader may observe a partially updated set.** One row gives a reader a consistent
   snapshot for free. Per-category rows or per-category files do not, unless the reader is given
   something to reconcile them with. Whether a half-updated list is acceptable on screen is a
   product question, and it decides between options B and C below more than any performance number
   does.

## Resolves into

[../decisions/](../decisions/), as a record on the cache's shape. It moves a boundary: the shape
determines whether a writer can refresh one category without touching the others, and changing it
afterwards is a migration rather than an edit.

## Source

Raised 2026-09-16, in a discussion of rewriting hub in Python. The rewrite framing is a separate
question, in [should-the-tui-be-rebuilt-in-textual.md](should-the-tui-be-rebuilt-in-textual.md);
the cache's shape is independent of the language it is written in.

## Options

- **A. Keep one SQLite row.** Strongest case: 150 lines that already work, with WAL and a 5000ms
  `busy_timeout` set (`apply_pragmas` in `store/src/status_cache.rs`) and covered by tests that
  assert the pragmas and that a second writer blocks rather than failing
  (`apply_pragmas_enables_wal_on_file_backed_connection`,
  `apply_pragmas_sets_busy_timeout_to_5000ms`,
  `second_writer_waits_for_open_write_transaction_instead_of_failing_immediately`). A reader always
  sees a consistent snapshot. Cost: no independent per-category write, and one `refreshed_at` for
  data of mixed age.
- **B. One SQLite row per category.** `id` becomes a category key instead of the literal `1`, and
  each row carries its own `refreshed_at`. Strongest case: the smallest change that fixes both
  problems, keeping WAL, `busy_timeout` and the option of a transaction across rows when a reader
  does need a consistent set. Cost: readers assemble the list from N rows and reconcile N
  timestamps, and `errors` needs a per-category home.
- **C. One JSON file per category, written by atomic rename.** Strongest case: removes rusqlite and
  the SQL entirely; a rename within a filesystem is atomic, so a partial write is never observable;
  the file's own mtime carries freshness without a column. Cost: no transaction across categories,
  so a reader can see a half-updated set with nothing to detect it by, and a directory scan replaces
  a query.
- **D. Not yet.** Strongest case: the shape is only wrong under an assumption that is itself an open
  question, and per-category cadence has not been decided. Cost: none today.

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it graduates
into a decision record._

- *Measured* (2026-09-16): the store is one table and one row. `CREATE TABLE` in
  `ensure_table`, the only read in `read`, the only write in `upsert`, all in
  `store/src/status_cache.rs`. No index, no
  second table, no join, no `ORDER BY`, no query that returns more than one row.
- *Measured* (2026-09-16): `StatusReport` is `{ items: Vec<StatusItem>, errors: Vec<String> }`
  (`StatusReport` in `workflows/src/status.rs`). `StatusItem` is an enum over PR, issue, CI, Linear,
  Loki and GCP variants plus private media variants, so a category is recoverable from an item but
  is not a key anything is stored under.
- *Sourced*: freshness is computed in Rust, not SQL. `read_if_fresh` compares `Utc::now() -
  refreshed_at` against a `max_age` argument (`read_if_fresh` in `store/src/status_cache.rs`), so a
  move away from SQL loses no query capability that is in use.
- *Sourced*: schema changes are handled by discard, not migration. `SCHEMA_VERSION` is an integer
  constant (`SCHEMA_VERSION` in `workflows/src/status.rs`, currently 18); a caller comparing it
  against the stored value drops the row and refetches on mismatch (the `schema_version` check in
  `ui/tui/src/main.rs`). Any option here inherits
  that mechanism rather than needing a new one.
- *Measured* (2026-09-16): diskcache is not a candidate. Its last release on PyPI is 5.6.3, uploaded
  2023-08-31. One reason disqualifies it: it trades 150 lines hub owns and understands for an
  unmaintained dependency. Reverses if it resumes releases and hub needs an eviction or expiry
  policy it would otherwise have to write.
- *Reasoned* (2026-09-19): the daemon in #336 reuses `status_cache::upsert` unchanged, so it becomes
  a second writer of the single row alongside `ui/tui/src/main.rs`. Option D is what milestone #327
  ships on. Whichever option eventually wins, the migration rewrites two write sites instead of one.
  That is the accepted cost of not deciding the shape on an inference about cadence.
- *Measured* (2026-10-03, read from the source): the 2026-09-19 inference is now fact. The daemon
  writes through `store::status_cache::upsert` unchanged (`write` in `daemon/src/cache.rs`), and
  `ui/tui/src/main.rs` still calls `upsert` as well, so the single row has two writers until Phase 5
  of [#327](https://github.com/ooloth/hub/issues/327). The table is still one row keyed `id = 1`.
- *Measured* (2026-10-05, read from the source): a pass where some sources fail still replaces the
  whole row, without the failed sources' items. `classify` in `daemon/src/freshness.rs` returns
  `Refreshed` whenever any item came back, and `write` in `daemon/src/cache.rs` upserts that report
  as the payload. If `github prs awaiting review` times out while other sources answer, those PRs
  leave the cached list until a later pass reaches that source. The TUI's partial banner names the
  source, and the health record in [#341](https://github.com/ooloth/hub/issues/341) will too, but
  the items are gone either way.
- *Reasoned* (2026-10-05): this does not depend on per-category cadence. On one global clock a
  partial pass still drops a source's last answer, and the Phase 4 notifications in
  [#327](https://github.com/ooloth/hub/issues/327) read this row, so a partial pass notifies about
  fewer PRs than are waiting. Options B and C keep a failed source's last answer by writing only the
  sources that answered. Option A can keep it only by merging the previous payload on every write.
- *Reasoned* (2026-10-08): the daemon's health record does not depend on this question.
  [Decision 026](../decisions/026-daemon-health-is-one-row-in-its-own-table.md) puts it in a table
  of its own, so neither the single-row shape nor a per-category one has to carry it.
