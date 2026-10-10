# daemon

The `hub-daemon` binary — hub's **unattended surface**. Refreshes the signal cache with nobody
present ([Decision 020](../docs/decisions/020-hub-runs-an-unattended-surface.md)).

**Rules:**

- An entry point, like `ui/`: bootstraps config, wires deps, calls workflows
- Sits beside `ui/` rather than inside it, because `ui/` means user interface and this has no user
- Every I/O call is written in `main`; nothing below it opens a socket or a database on its own

**Lives here:** the refresh sequence, the rule for when a refresh may replace the cache, and
recording each pass: the payload when it replaced it, and the health record always.

## What it does today

A startup line, then a pass, then one every 15 minutes (`--interval` changes it), each logged as one
line and recorded in the health record. One daemon per profile: a second refuses, naming the first.
`--once` runs a single pass and exits. `just daemon-start` runs the installed binary under launchd
for the `default` profile, at every login, logging to `~/.hub/default/daemon.log`. Everything else
in [#327](https://github.com/ooloth/hub/issues/327) is a later phase: credentials with nobody
present (3.6), notifications (Phase 4), and the socket the TUI will read, which is also where health
is first shown (Phase 5).

## Files

- `main.rs` — parses flags, takes the lock, prints the startup line, loads config once, then runs
  passes. Every I/O call is here, including each pass's fetch and write
- `startup.rs` — `Startup`, the line printed before credentials load. Pure
- `instance_lock.rs` — one daemon per profile: the `flock` on `~/.hub/<profile>/daemon.lock`
- `schedule.rs` — `every`, which runs a pass now and then once per period, never catching up
- `pass.rs` — `PassReport`, the per-pass log line, and what `--once` exits with. Pure. The pass
  itself, `domain::daemon_pass::Pass`, lives in `domain/` so the store can record it
- `refresh.rs` — `params` and `fetch`, which asks all sources once and merges the answers. The
  network edge
- `freshness.rs` — `RefreshOutcome`, `FreshStatus`, `FreshPayload`, `classify`. Pure; no I/O
- `cache.rs` — `open` and `record`, which writes the pass and its payload in one transaction and
  falls back to recording the pass as failed. The SQLite edge

`refresh::fetch` takes no database handle on purpose: `rusqlite::Connection` is not `Sync`, so
holding one across the fetch would make the surrounding future non-`Send` and unspawnable, which
the interval loop (`schedule::every`) needs and the socket server will.

## The rule that makes this more than a wrapper

`workflows::status::run` returns `Ok` whether or not anything answered. A failing source goes
into `Refresh::sources` with its reason and is named in `StatusReport::errors`, while the rest
still contribute items. So a total outage produces a well-formed report with an empty item list,
and writing that over a populated cache is silent data loss. A source that does not answer within
60 seconds (`SOURCE_TIMEOUT`, `workflows/src/status.rs`) is reported as failed like any other, and
the others still contribute.

A refresh is cached unless it came back with nothing **and** a source failed:

- items, no failures → written
- items, some failures → written (partial)
- nothing, no failures → written (the queue genuinely drained, and the cache has to say so)
- nothing, some failures → refused, previous row untouched, logged as `payload=kept` with
  `outcome=no_source_answered`, or `outcome=partial` when some sources answered with nothing

The conjunction matters in both directions. See
[the invariant](../docs/invariants/a-refresh-that-reached-no-source-never-replaces-the-cache.md)
for what enforces it and what the enforcement misses.

## Running it

```bash
just daemon                    # a pass now, then every 15 minutes, until stopped
just daemon --interval 20s     # the same, every 20 seconds
just daemon --once             # one pass, then exit
```

Expect fingerprint prompts at startup if 1Password has not been touched recently: every `op://`
reference resolves once, before the first pass, and later passes reuse them. Each start asks again,
including a restart, so a daemon that restarts while nobody is there waits on prompts nobody
answers.

Before loading credentials the daemon prints one line, so a start with no pass line after it is a
daemon waiting on 1Password:

```
hub-daemon start at=2026-10-03T23:15:55Z pid=4242 profile=default
```

Each pass prints one line to stdout:

```
hub-daemon pass at=2026-10-03T23:15:55Z pid=4242 profile=dev outcome=partial payload=written duration_ms=6412 items=1066 answered=9 failed_sources=1 failed="private workflows: connection refused"
```

`outcome` says how far the pass reached: `ok` (every source answered), `partial` (some failed, at
least one answered), `no_source_answered` (every source failed) or `pass_failed` (the pass itself
failed, with `error`). `payload` says whether the cache was replaced: `written`, or `kept` when
nothing that answered had any signals and something failed. `pid` changes when the daemon
restarts.

When the database refuses the write, the line ends with `recorded=as_failed record_error="..."`:
the pass was recorded as `pass_failed` instead, health only. When it refuses that too, the line
ends with `recorded=no`, both errors, and the health record keeps its previous pass.

## Running it under launchd

Opt-in per device. `just install` installs `hub-daemon` beside `hub-tui`, and these run it for the
`default` profile, the one the installed TUI reads:

```bash
just daemon-start   # install the LaunchAgent and start the daemon; it starts again at every login
just daemon-logs    # follow ~/.hub/default/daemon.log
just daemon-stop    # stop it until the next login; the LaunchAgent stays installed
```

`just daemon-start` writes `~/Library/LaunchAgents/com.ooloth.hub.daemon.plist`
(`scripts daemon start`, in `scripts/src/daemon_agent/`). The plist holds absolute paths, because
launchd expands neither `~` nor variables:

- the binary: the first `hub-daemon` on the caller's `PATH`
- the working directory: the main checkout, even from a worktree, since `hub.toml` is read from it
- `PATH`: the directories where the caller's shell finds `git`, `op` and `gcloud`, then
  `/usr/bin:/bin:/usr/sbin:/sbin`. See
  [the invariant](../docs/invariants/every-program-the-daemon-runs-is-on-its-launchd-path.md)
- stdout and stderr: both to the log, appended and never rotated, at about 280 bytes per pass

It sets no `HUB_PROFILE`, so the daemon uses `default` whatever the calling shell exports.
`KeepAlive` restarts a daemon that exits, at most once a minute (`ThrottleInterval` 60), so a
daemon that cannot start leaves one failure in the log per minute.

Rerunning `just daemon-start` with nothing changed does nothing. It writes the plist only when its
bytes would change, and reloads a running daemon only then, because macOS posts a Background Items
notification whenever the plist is rewritten, even with identical content.

**macOS can disallow the daemon** from System Settings > General > Login Items & Extensions,
where it is listed as `hub-daemon`. `just daemon-start` checks that launchd loaded the job after
starting it, and fails naming Login Items when it did not. Whether a disallowed job reaches that
check, or fails earlier in `launchctl bootstrap`, has not been observed. Nothing reports a daemon
disallowed later until Phase 5 shows health in the TUI.

## Observing it work

**It wrote the cache.** Run this before and after; `refreshed_at` should jump to the second the
daemon ran, and there is only ever one row.

```bash
sqlite3 ~/.hub/dev/hub.db "SELECT schema_version, refreshed_at, length(payload) FROM status_cache WHERE id=1;"
sqlite3 ~/.hub/dev/hub.db "SELECT count(*) FROM status_cache;"   # always 1
```

**What it recorded about the pass.** The health record is one row, rewritten every pass, and
`started_at` moves on even when the payload is kept.

```bash
sqlite3 -line ~/.hub/dev/hub.db "SELECT * FROM daemon_health;"
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
cargo run -p hub-daemon --features private -- --once; echo "exit $?"
```

Expect `outcome=no_source_answered payload=kept` naming every failed source, exit 1,
`refreshed_at` untouched, and the health record's `started_at` moved to this pass. Without
`--once` the same line repeats each pass and the daemon keeps running.

**Exit codes with `--once`**: 0 wrote, 1 did not. A second daemon for the same profile also exits 1,
naming the running one.

```bash
cargo run -q -p hub-daemon --features private -- --once >/dev/null 2>&1; echo $?
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
