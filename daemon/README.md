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
for the `default` profile, at every login, logging to `~/.hub/default/daemon.log`. When the daemon
cannot obtain its credentials, it records who was not answering and tries again in the same
process: within about a second after a prompt went unanswered or `op` reached its 90-second
deadline, so a fresh prompt is on screen, and otherwise waiting a second, then doubling up to a
minute. When an attempt finds the 1Password app not running, the daemon opens it (`open -a
1Password`), at most once a minute, so someone returning sees it asking to be unlocked. It never
closes or quits the app. After 10 minutes without credentials it posts a macOS notification through
`terminal-notifier`, saying signals have paused and what's needed, then again every hour while they
stay missing. Each replaces the last, and once credentials arrive the daemon removes it. Everything
else in [#327](https://github.com/ooloth/hub/issues/327) is a later phase: notifications about pull
requests (Phase 4), and the socket the TUI will read, which is also where health is first shown
(Phase 5).

## Files

- `main.rs` — parses flags, takes the lock, prints the startup line, reads `hub.toml`, obtains
  credentials once, then runs passes. Every I/O call is here, including each pass's fetch and write
- `credentials.rs` — `obtain`, which resolves credentials until they arrive, `Backoff`, the waits
  between attempts, and `AttemptReport`, the log line for a failed attempt. No I/O of its own
- `startup.rs` — `Startup`, the line printed before credentials load. Pure
- `instance_lock.rs` — one daemon per profile: the `flock` on `~/.hub/<profile>/daemon.lock`
- `schedule.rs` — `every`, which runs a pass now and then once per period, never catching up
- `pass.rs` — `PassReport`, the per-pass log line, and what `--once` exits with. Pure. The pass
  itself, `domain::daemon_pass::Pass`, lives in `domain/` so the store can record it
- `refresh.rs` — `params` and `fetch`, which asks all sources once and merges the answers. The
  network edge
- `freshness.rs` — `RefreshOutcome`, `FreshStatus`, `FreshPayload`, `classify`. Pure; no I/O
- `cache.rs` — `open` and `record`, which writes the pass and its payload in one transaction and
  falls back to recording the pass as failed, and `record_credential_attempt`. The SQLite edge

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

Before resolving credentials the daemon prints one line, so a start with nothing after it is a
daemon in its first `op read`, which ends within 90 seconds:

```
hub-daemon start at=2026-10-03T23:15:55Z pid=4242 profile=default
```

A failure before the first pass prints one line to stderr and exits 1. The line omits `profile`
when `HUB_PROFILE` itself was the failure:

```
hub-daemon start_failed at=2026-10-10T23:14:30Z pid=1048 profile=default error="hub-daemon is already running for profile default (pid 829, ...)"
```

Each attempt to resolve credentials that fails prints one line to stdout and is recorded in the
health record, with `cause` one of:

- `touch_id_prompt_unanswered`: 1Password's Touch ID prompt went unanswered, and `op` gave up after
  60 seconds. A Touch ID prompt on screen at the 90-second deadline gets up to 65 seconds more, so
  it isn't cut short
- `macos_prompt_waiting_for_you`: `op` had not answered after 90 seconds and a macOS alert is on
  screen, under launchd usually "hub-daemon would like to access data from other apps"
- `app_running_not_answering`: `op` had not answered after 90 seconds, no alert is on screen, and
  the 1Password app is running
- `app_not_running`, `unreachable` or `other`

When the daemon opened the app after an attempt, the line also says `opened_app=yes`, or
`opened_app=failed` with `open_error`, and the health record keeps the same in `opened_app` and
`open_error`. When it posted a notification, the line says `notified=yes`, or `notified=failed` with
`notify_error`, kept in `notified` and `notify_error`. Removing it once credentials arrive leaves a
`hub-daemon notification_removed` line, or `notification_remove_failed` with the error.

`retry_in` is absent with `--once`, which makes one attempt and exits 1:

```
hub-daemon credentials at=2026-10-12T08:59:03Z pid=4242 profile=default attempt=3 cause=app_not_running duration_ms=812 retry_in=4s error="failed to resolve credential from 1Password: ... couldn't connect to the 1Password desktop app."
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
just daemon-start      # install the LaunchAgent and start the daemon; it starts again at every login
just daemon-logs       # follow ~/.hub/default/daemon.log
just daemon-restart    # restart it, such as after editing hub.toml; fails when it is not running
just daemon-stop       # stop it until the next login; the LaunchAgent stays installed
just daemon-uninstall  # stop it and remove the LaunchAgent, so it no longer starts at login
```

`just install` ends by restarting a running daemon, so it runs the binary just installed. A stopped
daemon stays stopped and loads the new binary at the next login, and a device that never ran
`just daemon-start` is left alone.

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
daemon that cannot start leaves one `start_failed` line in the log per minute.

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
