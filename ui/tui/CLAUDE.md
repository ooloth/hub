# TUI

## Verifying TUI changes via insta + tmux

TUI verification has two tiers depending on what changed.

**Tier 1 — snapshot tests (rendering and layout changes)**

Full-screen `insta` snapshots cover all major screen states (see
`ui/tui/README.md` for the full list and conventions). If a rendering
change causes a visual regression, a snapshot diff will show exactly what
changed. Run `just test` and review any failures.

If the diff is intentional, accept it:

```bash
just test-update
```

When adding a new screen state or item type, add a snapshot for it —
don't rely on the existing snapshots to catch regressions in new code
paths.

**Tier 2 — tmux E2E (interaction and behavior changes)**

For changes that affect keybindings, navigation between screens,
subprocess launching, tmux integration, store schema, cache format, or
domain types the TUI deserializes on startup, snapshots are not sufficient.
Run the TUI live in tmux and drive the interaction:

1. Start the TUI in tmux.
2. Drive the changed keybinding or interaction with `tmux send-keys`.
3. Capture the pane with `tmux capture-pane -p`.
4. If the change launches another pane, window, browser, shell command, or
   external process, verify that launch behavior live.
5. Clean up any tmux panes/windows created during the test.
6. Report exactly what was observed.

```bash
tmux new-window -n "tui-test" "just tui; read"
sleep 3                                          # wait for data to load
tmux send-keys -t "tui-test" "?" ""             # send a keystroke
sleep 0.5
tmux capture-pane -t "tui-test" -p              # read the screen
tmux kill-window -t "tui-test"                  # clean up
```

**tmux send-keys pitfalls**

- Use named keys for special keys: `"Enter"`, `"Escape"`, `"Backspace"`,
  `"Up"`, `"Down"`, `"Tab"`. An empty string `""` sends **nothing** — it is
  not a shorthand for Enter.
- When testing the filter query flow: commit the query with `"Enter"` before
  pressing `"/"` again. If the query is not committed the TUI stays in query
  mode and the second `"/"` is treated as `AppendQuery('/')`, not `StartQuery`.

If E2E validation cannot be run, explicitly state why and what weaker
validation was run instead. Before concluding it cannot be run, verify
by inspection: read the config, check the relevant directories, confirm
what is actually available. Never assume a prerequisite is missing.
