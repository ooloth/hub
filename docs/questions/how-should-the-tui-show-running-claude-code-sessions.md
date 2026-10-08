---
opened: 2026-10-08
status: open
resolves_into: decision
---

# How should the TUI show running Claude Code sessions?

## Why it matters

Several Claude Code sessions run at once across tmux sessions, some started by `i` and some started
by hand. Losing track of which ones are busy, which are waiting for me, and which have finished
leaves work stalled without anyone noticing. Hub sits on its own screen and already answers "what
needs my attention", so it is the place I would look. Showing sessions the wrong way repeats what
[Decision 019](../decisions/019-drop-task-model-filesystem-sessions.md) removed: rows "that compete
with the signal rows they shadow".

## What would settle it

Two things, in this order. First, reading `~/.claude/sessions/*.json` for a while shows whether its
`status` is accurate enough to act on: whether it ever reports waiting for permission separately
from idle, whether files outlive a crashed process, and whether the `tmux` field stays correct after
a window moves. If it proves too buggy, the question closes with nothing built. If it holds up, a
prototype of each layout on the third screen shows which one I read at a glance.

## Resolves into

[../decisions/](../decisions/), because reading Claude Code's undocumented session files is a
coupling of the kind [Decision 015](../decisions/015-accept-tmux-and-claude-code-coupling.md)
weighs.

## Source

Raised 2026-10-08 while setting up two Ghostty splits on one monitor, each attached to a different
tmux session, with hub on a third screen.

## Options

- **A. Sessions as rows in the existing ranked list.** Strongest case: hub stays one prioritized
  view, and an idle session ranks high because it waits on me while a busy one ranks low. Cost:
  session rows mix with pull requests, so seeing all sessions at once needs a filter key, and a
  session started by `i` duplicates its signal's row, which is what 019 removed.

  ```
  ╭ hub ─────────────────────────────────────────────────────────────────────────────────────────╮
  │Claude · idle · api-service:@4 · api-service-2a                                            12m│
  │PR · Add retries to the nightly export job #228 · no reviews · blocked   acme/api-service · 2d│
  │PR · Fix the flaky login test #232 · no reviews · blocked                   acme/web-app · 22h│
  │Claude · busy · web-app:@18 · web-app-6f                                                    3m│
  │Claude · busy · dotfiles:@2 · dotfiles-91                                                   1m│
  │PR · Document the staging deploy #531 · no reviews · blocked            acme/api-service · 16h│
  ╰──────────────────────────────────────────────────────────────────────────────────────────────╯
   6/6 · [↩] details · [a] attach · [c] claude · [p] prs · [/] search               updated 1m ago
  ```

- **B. A separate sessions panel above the list.** Strongest case: every session is always in the
  same place, which is easier to scan from another screen, and signal rows are left alone. Cost: a
  second list next to the ranked one, so urgency is compared in two places.

  ```
  ╭ sessions ────────────────────────────────────────────────────────────────────────────────────╮
  │● idle  api-service  @4   api-service-2a                                                   12m│
  │○ busy  web-app      @18  web-app-6f                                                        3m│
  │○ busy  dotfiles     @2   dotfiles-91                                                       1m│
  ╰──────────────────────────────────────────────────────────────────────────────────────────────╯
  ╭ signals ─────────────────────────────────────────────────────────────────────────────────────╮
  │PR · Add retries to the nightly export job #228 · no reviews · blocked   acme/api-service · 2d│
  │PR · Fix the flaky login test #232 · no reviews · blocked                   acme/web-app · 22h│
  │PR · Document the staging deploy #531 · no reviews · blocked            acme/api-service · 16h│
  ╰──────────────────────────────────────────────────────────────────────────────────────────────╯
   [↩] attach · [tab] switch panel · [p] prs · [/] search                           updated 1m ago
  ```

- **C. Show no sessions.** Strongest case: nothing depends on an undocumented file. Cost: tracking
  sessions stays in my head.

Both A and B need an attach action to be useful, and hub has none yet.

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it graduates
into a decision record._

- *Measured* (2026-10-08, Claude Code 2.1.293 and 2.1.294): each running session writes
  `~/.claude/sessions/<pid>.json` with `status`, `tmux` (for example `web-app:@18.%19`), `name`,
  `cwd`, `updatedAt` and `statusUpdatedAt`. The three live files showed only `busy` and `idle`, so
  a separate waiting-for-permission state is unconfirmed.
- *Sourced* ([Decision 015](../decisions/015-accept-tmux-and-claude-code-coupling.md)): the same
  files were the fallback for task status detection, and 015 calls them an undocumented internal
  API. 019 dropped the task model that used them and keeps 015's tmux coupling.
- *Sourced* ([Decision 019](../decisions/019-drop-task-model-filesystem-sessions.md)): the
  `AgentSession` row was removed from the unified list along with the task model, because the task
  added a second lifecycle for work a signal already represents. Sessions started by hand have no
  signal, so that reason covers only some of the rows in option A.
- *Sourced* (`ui/tui/src/tmux.rs`): hub lists and selects windows in its own tmux session only.
  Attaching to a session elsewhere needs `switch-client` on one of the other tmux clients, and hub
  has to choose which one.
