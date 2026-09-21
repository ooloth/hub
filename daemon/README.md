# daemon

The `hub-daemon` binary — hub's **unattended surface**. Refreshes the signal cache with nobody
present ([Decision 020](../docs/decisions/020-hub-runs-an-unattended-surface.md)).

**Rules:**

- An entry point, like `ui/`: bootstraps config, wires deps, calls workflows
- Sits beside `ui/` rather than inside it, because `ui/` means user interface and this has no user
- Every I/O call is written in `main`; nothing below it opens a socket or a database on its own

**Lives here:** the refresh sequence, the rule for when a refresh may replace the cache, the write.

## What it does today

One refresh, then exit. Everything else in [#327](https://github.com/ooloth/hub/issues/327) is a
later phase: looping and the single-instance guard (3.3), the health record (3.4), credential
retry (3.5), launchd and log files (3.6), notifications (Phase 4), and the socket the TUI will read
(Phase 5).

## Files

- `main.rs` — parses flags, then performs every I/O call in sequence: load config, fetch, write
- `refresh.rs` — `fetch`, which asks all sources once and merges the answers. The network edge
- `freshness.rs` — `RefreshOutcome`, `FreshStatus`, `classify`. Pure; no I/O; all the unit tests
- `cache.rs` — `write` and `apply`. The SQLite edge

`refresh::fetch` takes no database handle on purpose: `rusqlite::Connection` is not `Sync`, so
holding one across the fetch would make the surrounding future non-`Send` and unspawnable, which
the interval loop and the socket server will both need.

## The rule that makes this more than a wrapper

`workflows::status::run` returns `Ok` whether or not anything answered. A failing source goes
into `StatusReport::errors` and the rest still contribute items, so a total outage produces a
well-formed report with an empty item list, and writing that over a populated cache is silent
data loss.

A refresh is cached unless it came back with nothing **and** a source failed:

- items, no failures → written
- items, some failures → written (partial)
- nothing, no failures → written (the queue genuinely drained, and the cache has to say so)
- nothing, some failures → refused, exit 1, previous row untouched

The conjunction matters in both directions. See
[the invariant](../docs/invariants/a-refresh-that-reached-no-source-never-replaces-the-cache.md)
for what enforces it and what the enforcement misses.

## Running it

```bash
just daemon
```

Expect a fingerprint prompt if 1Password has not been touched recently — three `op://` references
resolve before anything else happens.

Success prints one line to stdout and exits 0:

```
hub-daemon refresh=ok items=1136 failed_sources=0 schema_version=18
```

`failed_sources` is the number to read. Zero means every source answered; non-zero means a partial
refresh, which is still written.

## Observing it work

**It wrote the cache.** Run this before and after; `refreshed_at` should jump to the second the
daemon ran, and there is only ever one row.

```bash
sqlite3 ~/.hub/dev/hub.db "SELECT schema_version, refreshed_at, length(payload) FROM status_cache WHERE id=1;"
sqlite3 ~/.hub/dev/hub.db "SELECT count(*) FROM status_cache;"   # always 1
```

**What it fetched.** Useful for confirming the private sources resolved, since those need the
`op://` credentials:

```bash
sqlite3 ~/.hub/dev/hub.db "SELECT payload FROM status_cache WHERE id=1;" | python3 -c "
import json,sys,collections
r = json.load(sys.stdin)
for k,v in collections.Counter(list(i)[0] for i in r['items']).most_common(): print(f'{v:6d}  {k}')
print('errors:', r['errors'])
"
```

**The TUI renders what the daemon wrote.** Note `refreshed_at`, open the TUI, quit, check again.
Unchanged means the TUI fetched nothing of its own.

```bash
sqlite3 ~/.hub/dev/hub.db "SELECT refreshed_at FROM status_cache WHERE id=1;"
just tui        # look, then q
sqlite3 ~/.hub/dev/hub.db "SELECT refreshed_at FROM status_cache WHERE id=1;"
```

**A failed refresh leaves the cache alone.** Point egress at a dead port, exempting 1Password so
credentials still resolve:

```bash
NO_PROXY=my.1password.com,.1password.com,1password.com \
HTTPS_PROXY=http://127.0.0.1:1 HTTP_PROXY=http://127.0.0.1:1 \
cargo run -p hub-daemon --features private; echo "exit $?"
```

Expect exit 1, every failed source named on stderr, and `refreshed_at` untouched.

**Exit codes**, which Phase 3.6 will key off: 0 wrote, 1 did not.

```bash
cargo run -q -p hub-daemon --features private >/dev/null 2>&1; echo $?
```

## Gotchas that cost time

- **Everything above runs against the `dev` profile**, because `just` exports `HUB_PROFILE=dev`
  and `cargo run` inherits it from the recipe. The installed hub's database at
  `~/.hub/default/hub.db` is untouched. To look at that one instead, set the variable for the
  invocation: `HUB_PROFILE=default just daemon`.
- **The TUI refetches if the cache is older than 30 minutes** (`REFRESH_INTERVAL_SECS`,
  `../ui/tui/src/main.rs`), and it is still a second cache writer until Phase 5. Check the daemon's
  work within that window, or the TUI will overwrite the row and it will look like the daemon did
  nothing.
- **Dropping `NO_PROXY` from the failure test exercises the wrong path.** The proxy also blocks
  `op read`, which reaches `my.1password.com` even with the 1Password desktop integration enabled,
  so the run fails at config load before any refresh happens. Exit 1 and an intact row either way,
  for two different reasons — read the error, not the exit code.
- **`op whoami` is not a test of whether credentials work.** With the desktop integration it
  reports "account is not signed in" while `op read` succeeds, because the integration authorises
  individual reads rather than creating a CLI session. Run the binary instead.
