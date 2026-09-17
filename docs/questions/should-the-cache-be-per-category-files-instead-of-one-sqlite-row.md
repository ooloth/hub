---
opened: 2026-09-16
status: open
resolves_into: decision
---

# Should the cache be per-category files instead of one SQLite row?

## Why it matters

The cache is one row. `store/src/status_cache.rs:83` creates a single table whose primary key is
always written as the literal `1`, holding one `schema_version`, one `refreshed_at`, and one
`payload` that is the whole serialized `StatusReport`. Every read is
`SELECT ... FROM status_cache WHERE id = 1` (`:129`). Every write is an upsert on the same key
(`:99`).

That shape assumes one refresh produces the whole world at one instant. Fetching each signal type on
its own cadence breaks the assumption in three places:

- **One timestamp cannot describe a payload of mixed age.** The TUI's "updated Xm ago" reads the
  single `refreshed_at`, and [Decision 022](../decisions/022-tui-reads-cache-never-fetches.md)
  makes saying how old the data is a requirement rather than a nicety. With per-category cadence,
  there is no single honest value to put there.
- **A per-category write has to rewrite everything.** `StatusReport` is
  `{ items: Vec<StatusItem>, errors: Vec<String> }` (`workflows/src/status.rs:87`) and `items` is
  flat, with no category partition. Refreshing one category means reading the blob, filtering that
  category's items out, splicing new ones in, and writing the whole thing back. That is a
  read-modify-write over every other category's data on every refresh of any one of them.
- **`errors` has the same problem.** A per-category refresh cannot clear only its own failure
  entries, because nothing in the list says which category produced which string.

[Decision 021](../decisions/021-daemon-owns-signal-refresh.md) makes the daemon the only writer,
which removes the cross-process race but not the coupling: one writer rewriting the whole blob per
category is still every category's data passing through every category's refresh.

## What would settle it

This cannot be answered before the cadence question in
[should-refresh-run-on-a-per-category-schedule.md](should-refresh-run-on-a-per-category-schedule.md)
is answered. If refresh stays on one global clock, the single-row shape is correct as it stands and
there is nothing here to decide. Everything below assumes that question resolves toward per-category
cadence.

Given that, two observations settle the shape:

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
  `busy_timeout` set (`store/src/status_cache.rs:71`) and covered by tests that assert the pragmas
  and that a second writer blocks rather than failing (`:216`, `:228`, `:238`). A reader always sees
  a consistent snapshot. Cost: no independent per-category write, and one `refreshed_at` for data of
  mixed age.
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

- *Measured* (2026-09-16): the store is one table and one row. `CREATE TABLE` at
  `store/src/status_cache.rs:83`, the only read at `:129`, the only write at `:99`. No index, no
  second table, no join, no `ORDER BY`, no query that returns more than one row.
- *Measured* (2026-09-16): `StatusReport` is `{ items: Vec<StatusItem>, errors: Vec<String> }`
  (`workflows/src/status.rs:87`). `StatusItem` is an enum over PR, issue, CI, Linear, Loki and GCP
  variants plus private media variants, so a category is recoverable from an item but is not a key
  anything is stored under.
- *Sourced*: freshness is computed in Rust, not SQL. `read_if_fresh` compares
  `Utc::now() - refreshed_at` against a `max_age` argument (`store/src/status_cache.rs:118`), so a
  move away from SQL loses no query capability that is in use.
- *Sourced*: schema changes are handled by discard, not migration. `SCHEMA_VERSION` is an integer
  constant (`workflows/src/status.rs:9`, currently 18); a caller comparing it against the stored
  value drops the row and refetches on mismatch (`ui/tui/src/main.rs:91`). Any option here inherits
  that mechanism rather than needing a new one.
- *Measured* (2026-09-16): diskcache is not a candidate. Its last release on PyPI is 5.6.3, uploaded
  2023-08-31. One reason disqualifies it: it trades 150 lines hub owns and understands for an
  unmaintained dependency. Reverses if it resumes releases and hub needs an eviction or expiry
  policy it would otherwise have to write.
