# 019 — Drop the task model; filesystem-based session tracking

> **⚠ Proposal for evaluation.** This document proposes superseding ADRs 011–016.
> It has not been implemented. An evaluating agent should weigh it against the
> direction those ADRs established and surface any consequences this document misses.

## Context

ADRs 011–016 built a task model intended to make delegated agent work visible,
resumable, reviewable, and mineable:

- **011** placed session and task tracking in the TUI process
- **012** defined `AgentTask` as a single type spanning the full lifecycle
- **013** established one-task-one-session, agent-only comments, deterministic
  session IDs
- **014** specified task workspaces, prompt injection, and session file polling
  for completion detection
- **015** accepted tight coupling to tmux and Claude Code
- **016** implemented fold-back — auto-transitioning tasks when their originating
  signals resolve (PR merged → task done, alert cleared → task done)

The investigation path (`i` key) predates this work and remained separate: it
creates a worktree, splits the tmux pane, runs Claude with injected context, and
cleans up on exit. No database record is written. This path was described by users
as feeling like magic.

The task model added substantial complexity atop the investigation infrastructure:

- An 8-state lifecycle (backlog → ready → in-progress → blocked → in-review →
  done / failed / cancelled) with transition guards and a 30-second dispatch tick
- A typed `TaskOrigin` enum encoding signal provenance, matched via `SignalIdentity`
  to badge signal rows
- Three fold-back variants (PR, issue, CI/alert) with debounce logic and
  per-platform batch fetching, implemented to prevent tasks from stalling when
  agents fail to call `hub task report`
- A `hub task report / comment / link` CLI protocol that agents are expected to
  call during sessions
- `AgentSession` rows in the TUI unified list, hidden by default to avoid
  duplicating the signal rows they shadow

In practice, the task lifecycle was not used as a daily driver. The investigation
path remained primary. The fold-back work — the largest single investment — is
crash-recovery logic dressed as a feature: it prevents tasks from getting stuck
when the agent does not report. The duplicate-signal problem (task row echoing
the signal row) required a hiding mechanism rather than pointing at a design flaw.

The core observation is this: **the signal is the unit of work**. A PR is what
is being reviewed. A CI failure is what is being fixed. An agent session is work
done *on* a signal. The Task abstraction adds a second identity (TASK-XXXX) for
the same underlying item, requires keeping that identity in sync with the signal's
state via fold-back, and produces rows in the TUI that compete with the signal
rows they shadow.

---

## Decisions

### Drop the Task abstraction entirely

The `Task` domain type, the `tasks` and `task_comments` tables, the task lifecycle
state machine, fold-back, the dispatch tick, and the `hub task *` CLI subcommands
are removed. No migration path is provided for existing task rows — the table is
dropped. The `AgentSession` `StatusItem` variant is removed from the unified list.
The `t` filter key, `n`/`N` task creation keys, and `s` status submenu are removed
from the TUI.

**Why:** The Task adds a second lifecycle for work already represented by a signal.
Every complexity in ADRs 012–016 traces back to keeping that second lifecycle
honest: fold-back synchronises it with the signal, `SignalIdentity` maps it back
to the signal for badging, the hiding mechanism prevents it from duplicating the
signal in the list. Removing the Task removes all of that coordination overhead.

**What is kept from prior decisions:**
- The worktree creation mechanics (already used by `i` investigations)
- `~/.hub/` as hub's system data home
- The per-signal-type investigation prompts and context injection
- The tmux coupling (015 is unchanged)
- The TUI owning the refresh loop and session tracking (011 Option B still holds;
  only the SQLite task tracking within it is dropped)

### `i` opens a detached named tmux window

The `i` keybinding changes from `tmux split-window -h` (attached split, one at a
time) to `tmux new-window -d -n <session-name>` (detached named window). The
session name encodes the signal identity in a human-readable form (e.g.
`hub-pr-ooloth-hub-159`, `hub-ci-ooloth-hub-test-suite`).

**Why:** A detached window enables concurrent sessions on multiple signals — the
user can press `i` on three signals in sequence and all three run simultaneously.
The named window is the primary handle: the TUI finds in-progress sessions by
scanning tmux window names; the user finds them in the tmux status bar.

The split-pane model assumed the user watches the session live. Detached windows
assume the user kicks off work and returns to the TUI. The `a` (attach) action
in the signal detail pane is how the user checks in on a specific session.

**What is lost:** The split-pane experience of watching investigation happen
alongside the TUI. This tradeoff is accepted in exchange for concurrent sessions
and a consistent model (all sessions are detached, regardless of signal type).

### Filesystem-based session records at `~/.hub/sessions/`

Each session launched via `i` creates a directory under `~/.hub/sessions/` at
the moment of launch. Directory naming is timestamp-first for natural sort order;
the exact convention is deferred but must be stable enough to scan and match
against signals.

Each session directory contains:

```
~/.hub/sessions/
  2026-06-20/
    ooloth-hub/
      pr-159/
        origin.toml     ← written at launch by hub: typed signal identity for TUI matching
        prompt.md       ← written at launch by hub: system prompt + user prompt sent to agent
        session-id.txt  ← Claude's session UUID, for locating the transcript at
                          ~/.claude/sessions/<uuid>.jsonl
        report.md       ← written by the agent before stopping main work
                          (expected but not enforced)
      ci-test-suite/
        ...
    ooloth-scripts/
      pr-55/
        ...
```

The corresponding tmux window name drops the org prefix for brevity: `hub-pr-159`,
`scripts-ci-test-suite`, `media-alert-oom-foo`. Repo names are unique enough in
practice that org qualification is unnecessary in the window bar.

The exact naming convention is deferred; the structure above illustrates the
intent. It must be stable enough for the TUI to reconstruct signal identity from
the directory path as a fallback if `origin.toml` is absent.

Signal identity for TUI matching is stored in `origin.toml` — a separate
machine-owned file distinct from the human-readable `prompt.md`. The TUI reads
only `origin.toml` on each refresh cycle to match session directories to signal
rows; it does not parse `prompt.md` for this purpose.

Keeping identity in its own file has two practical benefits. First, the TUI
scans `~/.hub/sessions/` on every refresh: reading a small structured TOML file
and deserialising it into a typed struct (the same pattern hub already uses for
`hub.toml`) is cheaper and less fragile than splitting frontmatter out of a
markdown document. Second, `origin.toml` and `prompt.md` have clearly separate
owners and purposes — `origin.toml` is written once at launch and never modified;
`prompt.md` is the exact record of what instructions and context hub sent to the
agent at launch, useful for a future reader or mining agent understanding what the
agent was told to do.

Directory names are not parsed for signal identity. This avoids brittle edge
cases with repo names that contain the same separator character used in the
directory scheme.

**Why files, not database rows:** The session record is naturally a set of
documents — context provided, output produced, transcript written by Claude. A
database row is a summary of a document set, not the document set itself. Storing
summaries in SQLite while the actual content lives in files adds a sync burden
with no query benefit at the scale of one user's sessions. File presence is the
state: report exists or it does not.

**Why `~/.hub/sessions/` and not inside worktrees:** Worktrees are transient.
Ephemeral worktrees are cleaned up when the split pane closes; PR worktrees are
cleaned up when the PR closes. Reports must outlive their worktrees to be useful
as a learning corpus. `~/.hub/sessions/` is permanent by default; cleanup policy
is deferred.

### Session state is binary: in-progress or done

With interactive `claude` (no `-p`), the tmux window never closes on its own when
the agent finishes a turn. "Waiting for input" and "actively working" are not
reliably distinguishable without agent cooperation.

The practical resolution: **`report.md` is the done signal.** When the agent
finishes its main work, it writes `report.md` and the session transitions to done.
The window stays open for follow-up questions. The TUI does not attempt to detect
internal session state beyond the file's presence.

Observable states:

| Files present | tmux window exists | State |
|---|---|---|
| `prompt.md`, no `report.md` | yes | in-progress |
| `prompt.md`, `report.md` | yes or no | done (window may still be open) |
| `prompt.md`, no `report.md` | no | abandoned (user closed window) |

The stall detection mechanism from 014 (polling `~/.claude/sessions/<pid>.json`
for `updatedAt` staleness → auto-transition to `blocked`) is not reproduced.
There is no `blocked` state. If the user is concerned a session has stalled, they
attach to it.

### TUI session visibility: badges, footer count, attach-from-detail

Sessions are surfaced as attributes of signal rows, not as independent rows:

**Signal row badge:** A signal row shows a small indicator when session
directories exist for it. The TUI scans `~/.hub/sessions/` on each refresh,
reads `origin.toml` from each directory to extract the signal identity, and
matches it to the current signal list. Possible badge states:
- dot (yellow): one or more sessions exist, none have a report yet
- checkmark (green): at least one session has a report
- count: if multiple sessions exist, show how many

**Global session visibility:** The exact mechanism for surfacing in-progress
sessions globally is flexible — options include a footer count, a dedicated
group at the top of the unified list, or a filter key that narrows the list to
signals with active sessions. The right choice will become clear from usage;
the constraint is that it must be lightweight and not require a separate sessions
panel or a new row type. "N in-progress sessions" visible at a glance is the
goal, so the user knows before closing their laptop whether anything is still
running.

**Detail pane (right panel):** When a signal has session history, the right
detail panel lists sessions newest-first with timestamp, status (done/in-progress/
abandoned), and — if `report.md` exists — its content inline or a link to open
it. For signals with multiple sessions (e.g. three rounds of investigation), all
sessions are listed; the user scrolls or pages through them. A tabbed view (one
tab per session) is a future option but not specified here.

**Attach action:** In the signal detail pane, a keybinding attaches to the
in-progress session's tmux window by name. If multiple sessions are in-progress
(unusual), attach targets the most recent.

**No sessions panel, no `t` filter:** Sessions are always encountered through the
signal they belong to. There is no independent session list and no `AgentSession`
row in the unified list.

### Session reports are hub's learning corpus

The full artifact set for one session:

1. `~/.hub/sessions/<dir>/prompt.md` — what hub provided to the agent
2. `~/.hub/sessions/<dir>/report.md` — what the agent produced
3. `~/.claude/sessions/<uuid>.jsonl` — the full transcript, written by Claude

These three files are the basis for future learning: a mining workflow reads
them, identifies where agents struggled, and proposes improvements to
investigation prompts. The corpus is never pruned; session directories accumulate
indefinitely by default (cleanup policy deferred to a future decision).

This supersedes the `agent-session-logs/` convention from 014 and the verdict
signal model from 017. Mining happens from raw files, not from structured DB
fields or summary rows.

### Investigation prompts are the primary investment

With the lifecycle machinery gone, the highest-leverage ongoing work is the
content of investigation prompts. The `i` session is only as useful as the
context it receives. Prompt grooming is expected to be iterative: each time an
investigation is weak or requires the agent to ask for context it should have
been given, the relevant prompt is updated.

Multi-repo context (where an error in one service traces to config in another)
is handled by including related-repo paths, URLs, and key file locations in the
investigation context, rather than by checking out multiple repos. Full multi-repo
workspace support (parallel worktrees under one session directory) is deferred.

---

## What this supersedes

| ADR | Status | Notes |
|---|---|---|
| 011 | Partially superseded | TUI single-process model is kept; SQLite task tracking is dropped |
| 012 | Superseded | Task model removed |
| 013 | Superseded | One-task-one-session model removed; sessions are now per-launch, not per-task |
| 014 | Partially superseded | Worktree creation mechanics kept; task dispatch tick, lifecycle, and `hub task *` CLI dropped |
| 015 | Unchanged | tmux and Claude Code coupling is still accepted as-is |
| 016 | Superseded | Fold-back removed; there are no tasks to fold back |
| 017 | Superseded for now | Verdict signal concept is preserved in spirit; mechanism is file-based mining, not DB fields |
| 018 | Superseded for now | Meta-loop concept is preserved; mechanism and timing are deferred |

---

## Consequences

**Code removed:**
- `domain/src/task.rs`, `task_origin.rs`, `signal_identity.rs`
- `store/src/tasks.rs`, `store/src/task_comments.rs`
- `workflows/src/task_fold_back.rs`, `workflows/src/dispatch.rs` (task dispatch
  portion; worktree creation helpers are kept or moved)
- `ui/tui/src/render/task.rs` and `Screen::NewTask` — the task creation modal is
  the only full-screen form modal in the TUI; its removal leaves the review
  submenu (`v`), merge confirmation (`m`), diff submenu (`d`), and query mode
  (`/`) untouched, as these are independent of the task system
- All `hub task *` CLI subcommands
- `tasks` and `task_comments` SQLite tables; `SCHEMA_VERSION` bumped
- `StatusItem::AgentSession` variant
- TUI keys: `t` (filter tasks), `n`/`N` (new task), `s` (status submenu)

**Code added:**
- `~/.hub/sessions/` directory creation at first launch
- Session directory creation at `i` launch (writes `origin.toml`, `prompt.md`, `session-id.txt`)
- TUI: filesystem scan on each refresh to match session dirs to signal rows
- TUI: signal row badge rendering from scan results
- TUI: footer in-progress session count
- TUI: detail pane right-panel session history and report display
- TUI: attach keybinding in detail pane
- Investigation prompt content updates to instruct agents to write `report.md`

**Open questions not resolved here:**
- Exact directory naming convention (deferred; timestamp-first is agreed,
  full scheme is authoring work)
- Whether `hub` CLI gains any session-writing helpers (e.g. `hub session report`)
  or whether agents write files directly with no hub CLI involvement
- Cleanup policy for `~/.hub/sessions/` directories
- Whether the `i` key retains a split-pane variant for cases where the user
  wants to watch the session live
