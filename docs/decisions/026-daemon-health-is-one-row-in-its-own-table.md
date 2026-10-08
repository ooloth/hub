---
number: 026
status: accepted
date: 2026-10-08
---

# 026 — The daemon's health is one row in its own table in the profile database

## Forced by

- [#341](https://github.com/ooloth/hub/issues/341)'s Ideal state: the daemon records the outcome of
  every pass where a reader holding only the database can find it, and a pass where every source
  failed keeps the cached payload but still updates health.
- [#339](https://github.com/ooloth/hub/issues/339): the daemon reports a credential failure into the
  health record. That failure happens before any pass, and on a first start before any cache row
  exists.
- The plan of record in [#327](https://github.com/ooloth/hub/issues/327): SQLite is the data store,
  and the socket carries control, not data.
- `status_cache` is one row whose `payload` is `NOT NULL` (`ensure_table` in
  `store/src/status_cache.rs`), and its shape is open in
  [the cache-shape question](../questions/should-the-cache-be-per-category-files-instead-of-one-sqlite-row.md).

## Decision

The daemon's health is a single row in a table of its own, in the profile's `hub.db`. It describes
the last pass only, and it is never stored in the `status_cache` row.

A separate row can exist before any payload does, so a credential failure on a first start has
somewhere to go. It keeps the health record's shape out of the cache-shape question, whichever way
that is answered. The daemon rewrites the row on every pass, so changing its schema means dropping
and recreating the table and loses at most one pass's record.

How the row is written alongside the payload, and its columns, belong to #341's design rather than
to this record.

## Rejected

- **Health columns on the `status_cache` row**, because the record has to exist before any payload
  does, and `payload` is `NOT NULL`. A placeholder payload breaks every reader that parses it.
  Reverses if the cache gains a row that legitimately holds no payload.
- **One row per pass, pruned to a limit**, because it settles a retention policy with no evidence
  of what history anyone needs. #341 scopes the record to the last pass, and the daemon's log under
  launchd ([#340](https://github.com/ooloth/hub/issues/340)) keeps pass history. Reverses if a
  reader needs past passes from the database, such as when a source last succeeded.
- **A `health.json` file in the profile directory, written by atomic rename**, because no
  transaction spans a file and SQLite, so a reader can see a fresh payload beside stale health with
  nothing to detect it by. Its strongest case is that it can still report a database that cannot be
  written. Reverses if health and payload no longer have to agree, such as if the payload leaves
  SQLite.
- **Not yet: the log is the record, and Phase 5 asks the daemon over the socket**, because a daemon
  that has died can then say nothing about its last pass to a reader of the database, and the plan
  of record keeps data out of the socket. Reverses if the socket comes to carry data.

## Risk

Health and payload are two rows, so a reader that reads them in separate statements can see one
pass's payload beside another pass's health. #341's store API has to make reading both together
the only way to read either. When the database cannot be written at all, the record cannot say
so. A reader learns of it only because the record's timestamp stops advancing.

## Revisit when

The cache-shape question resolves into per-source rows that each carry their own age and failure,
which would make a separate record of the pass redundant. Or a reader needs pass history from the
database.

## Also update

- [x] questions/README.md — nothing settled or re-scoped. The cache-shape question gains a finding
      that the health record does not depend on it.
- [x] vision.md — nothing defined or foreclosed.
