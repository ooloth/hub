# ui/tui

The `hub-tui` binary. A Ratatui terminal dashboard that renders status items
grouped by category, auto-refreshes from SQLite, and opens items in the
browser via keyboard shortcuts.

## Architecture

On launch: load config → open SQLite → read cache. If the cache is fresh
(≤ 30 min, matching `SCHEMA_VERSION`), render immediately from it. Otherwise
render an empty list and kick off a live fetch in the background.

The event loop runs on tokio and multiplexes three sources with `tokio::select!`:

- `crossterm::event::EventStream` — keyboard events
- `tokio::time::interval` (30 min) — periodic background refresh
- `mpsc::Receiver<Result<StatusReport>>` — results from background fetches

Background fetches run in spawned tokio tasks and send results back over the
channel. The `rusqlite::Connection` stays in the main task (it is not `Send`);
tasks return a typed `StatusReport`, and the main task serializes and upserts
it.

## Cache and schema version

The cache is a single SQLite row (see `store::status`). `SCHEMA_VERSION` is a
constant in `workflows::status`. When the TUI reads a cached row, it checks
`schema_version == SCHEMA_VERSION` before deserializing. A mismatch triggers a
live fetch, discarding the stale row.

**Bump `SCHEMA_VERSION` whenever `StatusReport` or any nested type changes in
a backward-incompatible way.** Forgetting to bump means old cached bytes get
deserialized into the new shape and will likely panic or produce garbage.

## Navigation

Three levels. `Enter` drills in; `Esc` backs out one level.

**Home** — category preview tiles

| Key        | Action                    |
| ---------- | ------------------------- |
| h          | left tile (or prev)       |
| j          | down tile (or next)       |
| k          | up tile (or prev)         |
| l          | right tile (or next)      |
| Tab        | next tile                 |
| Shift-Tab  | prev tile                 |
| Enter      | drill into category       |
| ?          | toggle help               |
| q / Ctrl-C | quit                      |

**Category** — full-screen item list

| Key        | Action                  |
| ---------- | ----------------------- |
| h          | up                      |
| j          | down                    |
| k          | up                      |
| l          | down                    |
| Enter      | open / drill into group |
| Esc        | back to home            |
| ?          | toggle help             |
| q / Ctrl-C | quit                    |

**Detail** — items within a group

| Key        | Action           |
| ---------- | ---------------- |
| h          | up               |
| j          | down             |
| k          | up               |
| l          | down             |
| Enter      | open URL         |
| Esc        | back to category |
| ?          | toggle help      |
| q / Ctrl-C | quit             |

## Terminal cleanup

The TUI enters raw mode and the alternate screen on launch, and restores both
on exit — even if an error is returned from the event loop. The restore
sequence runs unconditionally before propagating any error.

## Running

```bash
just tui          # build and run (no secrets)
op run --env-file=.env -- cargo run -p hub-tui --features private
```

The `private` feature enables private workflow integrations (Sonarr, etc.).
Without it, only public integrations (GitHub, Linear) are shown.
