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

## State machine

Keyboard events flow through a pure Elm-style pipeline before any state
is mutated:

1. `key_to_action(app, key) -> Option<Action>` (`input.rs`) — maps a raw
   key event to an `Action` variant. Takes the current view and `show_help`
   flag into account to resolve context-sensitive bindings (e.g. `h` maps
   to `MoveTileLeft` on Home but `MoveUp` on list views). Returns `None`
   for unmapped keys.
2. `App::update(action) -> Effect` (`state.rs`) — applies the action to
   app state and returns an `Effect` describing any required side effect
   (`OpenUrl`, `LaunchCi`, `Quit`, or `None`).
3. The event loop in `main.rs` handles the `Effect` — opens a URL,
   spawns a tmux split, or breaks the loop.

**Adding a new interactive behavior:**

1. Add a variant to `Action` in `state.rs`
2. Map one or more keys to it in `key_to_action()` in `input.rs`
3. Handle it in `App::update()` in `state.rs` — mutate state and/or return an `Effect`
4. If a new `Effect` variant is needed, handle it in `run_loop()` in `main.rs`

## Render model

Render functions take `&mut Frame` and cannot be unit tested in isolation.
Decision logic is extracted into pure helper functions that can be:

- `status_bar_left(app)` (`render/mod.rs`) — computes the left status bar
  string from the current view state and selected item
- `hint_for_category_item(item)` (`render/category.rs`) — returns the
  action hint text for the selected item in a category list
- `hint_for_detail_item(item)` (`render/detail.rs`) — same for detail view

Visual constants live in `render/mod.rs` and are accessed by submodules
via `super::`:

| Constant/function  | Value                        | Used for                        |
| ------------------ | ---------------------------- | ------------------------------- |
| `FOCUS_COLOR`      | `Color::Green`               | focused borders and titles      |
| `SELECTION_BG`     | `Color::Rgb(41, 45, 62)`     | list selection background       |
| `dim()`            | `Modifier::DIM`              | secondary and unfocused text    |
| `list_highlight()` | `SELECTION_BG` + `BOLD`      | stateful list widget highlight  |
| `urgency_style(u)` | red / yellow / default / dim | urgency-coloured bullet dots    |

**Adding a new view:**

1. Add a variant to `View` in `state.rs`; include a `ListState` field if
   the view has a scrollable list
2. Push the new view from `App::update(Action::Enter)` when appropriate
3. Add a render function in `render/`; extract any decision logic (what to
   show, what hint to display) into a pure function alongside it
4. Dispatch to it in `render()` in `render/mod.rs`

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

| Key        | Action               |
| ---------- | -------------------- |
| h          | left tile (or prev)  |
| j          | down tile (or next)  |
| k          | up tile (or prev)    |
| l          | right tile (or next) |
| Tab        | next tile            |
| Shift-Tab  | prev tile            |
| Enter      | drill into category  |
| ?          | toggle help          |
| q / Ctrl-C | quit                 |

**Category** — full-screen item list

| Key        | Action                          |
| ---------- | ------------------------------- |
| h          | up                              |
| j          | down                            |
| k          | up                              |
| l          | down                            |
| Enter      | open / drill into group         |
| i          | investigate CI failure (CI only) |
| Esc        | back to home                    |
| ?          | toggle help                     |
| q / Ctrl-C | quit                            |

**Detail** — items within a group

| Key        | Action                          |
| ---------- | ------------------------------- |
| h          | up                              |
| j          | down                            |
| k          | up                              |
| l          | down                            |
| Enter      | open URL                        |
| i          | investigate CI failure (CI only) |
| Esc        | back to category                |
| ?          | toggle help                     |
| q / Ctrl-C | quit                            |

## Terminal cleanup

The TUI enters raw mode and the alternate screen on launch, and restores both
on exit — even if an error is returned from the event loop. The restore
sequence runs unconditionally before propagating any error.

## Running

```bash
just tui          # build and run (with secrets and hub-private workflows if installed)
```
