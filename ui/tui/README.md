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
   key event to an `Action` variant. Universal keys (quit, help, back) are
   handled first; the remainder delegates to a per-view function
   (`home_keys` or `list_keys`). Returns `None` for unmapped keys.
2. `App::update(action) -> Vec<Effect>` (`state.rs`) — applies the action
   to app state and returns zero or more `Effect` values describing required
   side effects (`OpenUrl`, `LaunchCi`, `Quit`).
3. The event loop in `main.rs` iterates the effects — opens a URL, spawns
   a tmux split, or breaks the loop. `Quit` short-circuits any remaining
   effects in the same vec.

**Adding a new interactive behavior:**

1. Add a variant to `Action` in `state.rs`
2. Map one or more keys to it in the appropriate per-view function in
   `input.rs` (`home_keys`, `list_keys`, or a new function for a new view)
3. Handle it in `App::update()` in `state.rs` — mutate state and/or return
   one or more `Effect` values
4. If a new `Effect` variant is needed, add it to the enum in `state.rs`,
   handle it in `run_loop()` in `main.rs`, and if it launches an
   investigation add a file in `investigations/` (see below)

## Render model

Render functions take `&mut Frame` and cannot be unit tested in isolation.
Decision logic is extracted into pure helper functions that can be:

- `status_bar_left(app)` (`render/mod.rs`) — computes the left status bar
  string from the current view state and selected item
- `hint_for_category_item(item)` (`render/category.rs`) — returns the
  action hint text for the selected item in a category list (wraps
  `item_hint` from `display.rs` and adds Group-specific handling)

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

1. Add a variant to `Screen` in `state/types.rs`; include a `ListState` field if
   the view has a scrollable list
2. Push the new view from `App::update(Action::Enter)` when appropriate
3. Add a render function in `render/`; extract any decision logic (what to
   show, what hint to display) into a pure function alongside it
4. Dispatch to it in `render()` in `render/mod.rs`
5. Add a full-screen snapshot test for the new view (see below)

## Snapshot tests

Render functions take `&mut Frame` and cannot be called from unit tests
directly. Tests use ratatui's `TestBackend` to render into a real buffer
and inspect the result as plain text.

Two helpers in `render/mod.rs` extract buffer content:

- `screen_text(buf)` — the full buffer as a multi-line string, one line
  per row. Use this for full-screen snapshots that cover borders, urgency
  dividers, item layout, and popup overlay.
- `status_row(buf)` — the last row only. Use this for focused tests on
  status bar content across terminal widths or frame transitions.

The test suite keeps one full-screen snapshot per major screen state:

| Snapshot | What it locks in |
| --- | --- |
| `full_screen_unified_list_empty` | bare border, no items, empty status bar |
| `full_screen_unified_list_mixed_urgency` | urgency divider between tiers |
| `full_screen_unified_list_group_selected` | group row with "↩ to expand" hint |
| `full_screen_unified_list_pr_selected` | item with no investigate action (shorter hint, "2/2" position) |
| `full_screen_unified_list_category_filter` | green border + category label in title |
| `full_screen_unified_list_committed_query` | green border + query text in title |
| `full_screen_unified_list_query_input` | yellow border while query is being typed |
| `full_screen_unified_list_narrow_terminal` | 40-col terminal, text wrapping |
| `full_screen_unified_list_scrolled` | 15 items, last selected, scroll offset visible |
| `full_screen_unified_list_help_popup` | keybind popup overlaid on list |
| `full_screen_detail_view_first_selected` | group expanded, first item selected |
| `full_screen_detail_view_last_selected` | group expanded, last item selected |

**Add a new snapshot whenever you introduce** a new screen variant, a new
item type that changes how a row renders (different hint, different dot
style), or a new layout mode. The snapshot is the acceptance test for
the visual change.

**Use a unit test instead** for pure functions (`wrap_text`,
`right_status_text`, `action_hints`) where the interesting variation is
parameterised logic or time-sensitive data that cannot go in a
deterministic snapshot.

**To update snapshots after an intentional change:**

```bash
INSTA_UPDATE=always cargo test -p hub-tui
```

## Investigations

Investigation types live in `investigations/`. Each type is a separate file;
`mod.rs` provides the shared `launch_in_tmux_split(command, cwd)` helper.

```
investigations/
    mod.rs      — shared tmux split-window launcher
    ci.rs       — CI failure investigation (github-ci-investigate skill)
```

Each file exposes a `launch(…context…, cwd)` function that builds a command
string and delegates to `launch_in_tmux_split`. The command string invokes a
Claude Code skill with the relevant context as arguments.

**Adding a new investigation type** (e.g. Grafana logs):

1. Add an `Effect` variant in `state/types.rs`: `LaunchGrafana { log_url: String }`
2. Add an `InvestigateAction` variant in `state/types.rs` and handle it in
   `compute_investigate_action()` in `state/update.rs`
3. Create `investigations/grafana.rs` with a `launch(log_url, cwd)` function
   and its unit test
4. Declare `pub(crate) mod grafana;` in `investigations/mod.rs`
5. Handle the new `Effect` variant in the `for effect in` loop in `run_loop()`
   in `main.rs`

## Cache and schema version

The cache is a single SQLite row (see `store::status`). `SCHEMA_VERSION` is a
constant in `workflows::status`. When the TUI reads a cached row, it checks
`schema_version == SCHEMA_VERSION` before deserializing. A mismatch triggers a
live fetch, discarding the stale row.

**Bump `SCHEMA_VERSION` whenever `StatusReport` or any nested type changes in
a way that would break deserialization of a cached row.** Forgetting to bump
means old cached bytes get deserialized into the new shape and will likely
panic or produce garbage.

Rules of thumb:
- Adding a new variant to `StatusItem` → bump (old cache won't have the tag)
- Removing or renaming a field → bump
- Changing a field's type → bump
- Adding a new optional field with `#[serde(default)]` → no bump needed
- Renaming the enum variant itself → bump

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
