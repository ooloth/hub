# Task Dispatch

This document describes how hub's TUI background loop detects ready tasks, claims
them atomically, and spawns Claude Code agents to work on them. It covers the state
machine, the component interactions, and the completion/stall detection mechanisms.

See [Decision 013](../decisions/013-task-session-model.md) and
[Decision 014](../decisions/014-task-dispatch.md) for the reasoning behind these
choices. See [tasks.md](tasks.md) for the task lifecycle and schema.

---

## State machine

Arrows show what triggers each transition and which system owns it.

```
HUMAN              TUI DISPATCH TICK (30s)   AGENT (claude)    ~/.claude/sessions/<pid>.json
  │                        │                      │                       │
  │ create task            │                      │                       │
  ├───────────────────────►│                      │                       │
  │                   [backlog]                   │                       │
  │                        │                      │                       │
  │ s→ready (promote)      │                      │                       │
  ├───────────────────────►│                      │                       │
  │                   [ready]                     │                       │
  │                        │                      │                       │
  │          in_progress < cap (1)?               │                       │
  │          oldest ready task?                   │                       │
  │          → atomic claim: status+session_id    │                       │
  │                   [in-progress] ─────────────►│                       │
  │                        │   tmux new-window -d │ process starts        │
  │                        │   -n TASK-XXXX       │ ─────────────────────► status="busy"
  │                        │   claude             │ (generating/tools)    │
  │                        │     --session-id     │                       │
  │                        │     --model opus     │                       │
  │                        │     ...              │                       │
  │                        │                      │                       │
  │  ┌─────────────────────┤◄── 10s session poll ─┼───────────────────────┤
  │  │ status="busy"       │                      │                       │
  │  │ updatedAt stale     │                      │                       │
  │  │ > 15 min            │                      │                       │
  │  ▼                     │                      │                       │
  │ [blocked] ─────────────┤ status="busy"again   │                       │
  │ (High urgency)         │ → self-heal          │                       │
  │                        │                      │ turn done             │
  │                        │                      │ ─────────────────────► status="idle"
  │                        │                      │                       │
  │          ┌─────── [PRIMARY] hub task report ──┘                       │
  │          │             │    → in-review (immediate)                   │
  │          │             │                                              │
  │          │◄── [FALLBACK] status="idle" > 30s → in-review ─────────────┤
  │          │             │                                              │
  │          │◄── [CRASH] session file absent → in-review ────────────────┤
  │          ▼             │                                              │
  │     [in-review]        │ (5 min buffer, then tmux kill-window)        │
  │          │             │ if task manually moved away within buffer,   │
  │          │             │ cancel the reap                              │
  │          │             │                                              │
  │ s→done/failed/cancel   │                                              │
  ├───────────────────────►│                                              │
  │          [done / failed / cancelled]                                  │
```

---

## Component interactions

```
 ┌───────────────────────────────────────────────────────────────────────┐
 │  TUI main loop (tokio::select!)                                       │
 │                                                                       │
 │  30s dispatch_interval ──► workflows::tasks::dispatch()               │
 │      └─ count in-progress tasks in SQLite (cap = 1)                   │
 │      └─ if under cap: fetch oldest ready task                         │
 │          └─ generate uuid5(TASK_NS, task_key)                         │
 │          └─ atomic SQL: claim task (status + session_id, one tx)      │
 │          └─ build prompt: read prompts/<kind>-task.md                 │
 │          └─ tmux new-window -d -n TASK-XXXX                           │
 │                 -e HUB_SYSTEM_PROMPT=<kind-prompt>                    │
 │                 -e HUB_TASK_PROMPT=<title+desc+links+comments>        │
 │                 "claude --session-id <uuid>                           │
 │                         --dangerously-skip-permissions                │
 │                         --model opus                                  │
 │                         --append-system-prompt \"$HUB_SYSTEM_PROMPT\" │
 │                         \"$HUB_TASK_PROMPT\""                         │
 │                                                                       │
 │  10s stream_interval ──► poll ALL in-progress tasks                   │
 │      └─ for each: scan ~/.claude/sessions/*.json for sessionId match  │
 │          ├─ status="busy", updatedAt stale >15m → blocked (auto)      │
 │          ├─ status="busy" fresh, was blocked → in-progress (heal)     │
 │          ├─ status="idle" >30s, no report → in-review (fallback)      │
 │          └─ file absent, no report → in-review (crash recovery)       │
 │      └─ selected task → update stream_blocks (existing behavior)      │
 │                                                                       │
 └───────────────────────────────────────────────────────────────────────┘
         ▲                                           │
         │                                           ▼
 ┌───────────────┐              ┌─────────────────────────────────────┐
 │  hub CLI      │              │  tmux window: TASK-XXXX             │
 │               │              │  claude --session-id <uuid>         │
 │  hub task     │◄─────────────│    --dangerously-skip-permissions   │
 │    report     │  (agent      │    --append-system-prompt <kind>.md │
 │  hub task     │   calls CLI) │    "TASK-XXXX: title..."            │
 │    comment    │              │                                     │
 │               │              │  writes JSONL to:                   │
 └───────────────┘              │  ~/.claude/projects/                │
         │                      │    <encoded-cwd>/                   │
         ▼                      │    <session-id>.jsonl               │
 ┌───────────────┐              └─────────────────────────────────────┘
 │  SQLite       │                              │
 │  tasks table  │◄─────────────────────────────┘
 │               │  (TUI polls ~/.claude/sessions/*.json per task)
 │  session_id   │
 │  status       │
 └───────────────┘
```

---

## Prompt injection

The dispatch command shares the `-e` env-var injection pattern from
`ui/tui/src/investigations/mod.rs` but differs in tmux command (`new-window -d`
vs `split-window -h`). See "Two session launch patterns" below.

- `--append-system-prompt "$HUB_SYSTEM_PROMPT"` — the content of
  `prompts/<kind>-task.md`, injected as a system prompt addition. This carries the
  kind-specific workflow instructions (how to approach an implement/debug/review task).

- `"$HUB_TASK_PROMPT"` — the positional `prompt` argument to `claude`. This is the
  first user message and carries task-specific context: title, description, links,
  agent comment thread, and the done-when instruction (`hub task report TASK-XXXX
--status in-review`).

Both are passed as tmux `-e` environment variables to avoid OS argument-size limits
and shell quoting complexity.

---

## Completion detection

Interactive `claude` (no `-p`) **never exits when the agent finishes a turn** — the
process stays alive waiting for the next message. Window disappearance therefore
detects only process death (crashes, manual kills), not normal completion. The
reliable signals are:

**Primary (fast, explicit)**:

- Agent calls `hub task report TASK-XXXX --status in-review` → immediate DB update

**Fallback (turn-complete detection)**:

- `~/.claude/sessions/<pid>.json` transitions `status: "busy"` → `status: "idle"` when
  the agent finishes a turn and is waiting for the next message
- TUI scans session files for the matching `sessionId`, polls `status` + `updatedAt`
- `status: "idle"` for >30s with no `in-review` DB update → auto-transition to `in-review`

**Crash recovery**:

- Session file disappears (process exited) with no `in-review` DB update → auto-transition

**Window reaping** (side-effect, not a DB state):

- When task transitions to `in-review`, TUI waits 5 minutes then kills the tmux window
- If task is manually moved away from `in-review` within the 5-minute window, reap is cancelled
- The session is always resumable via `claude --resume <session-id>` (stored in `tasks.session_id`)

**`~/.claude/sessions/` is an undocumented internal API.** Anthropic could change
format or location without notice. The primary signal (`hub task report`) is
independent of it. If polling fails, tasks stay in `in-progress` until manual correction.

---

## Stall detection

Hub detects stuck sessions via the session file's `updatedAt` timestamp (epoch-ms,
updated on every `status` change):

- `status: "busy"` AND `updatedAt` stale >15 minutes → auto-transition to `blocked`
  (High urgency, visible in unified list)
- `status` becomes `"busy"` again (fresh `updatedAt`) → auto-transition back to
  `in-progress` (self-heal)

15 minutes is chosen to avoid false positives from long-running tool calls (Rust
builds, slow clones, extended thinking). The human can switch to the `TASK-XXXX`
tmux window to investigate a genuinely stuck session.

`--dangerously-skip-permissions` minimizes permission stalls — most file and shell
operations are auto-approved for task sessions.

---

## Worktree isolation

Task sessions run in a **persistent named worktree**, not the human's main checkout.
All hub system data lives under `~/.hub/`:

```
~/.hub/
  repos/                       ← bare git clones + managed worktrees (existing)
    hub/
      main/                    ← default-branch worktree
      pr-123/                  ← PR investigation worktrees
  workspaces/                  ← task agent workspaces (new)
    TASK-0001/
      hub/                     ← git worktree on branch agent/TASK-0001
    TASK-0042/
      hub/                     ← git worktree for another task
  agent-session-logs/          ← agent completion reports (new)
  transcripts/                 ← existing session transcripts
  hub.db                       ← database (moving from macOS default)
```

Task workspaces are **separate from** `repos/`. The `repos/` subtree is managed by
the fetch loop and `clean_merged_worktrees`; mixing task worktrees there would expose
them to that loop's cleanup logic.

- Branch: `agent/TASK-XXXX` (checked out fresh from trunk at first dispatch)
- Worktree: `~/.hub/workspaces/TASK-XXXX/<project>/`
- Survives session termination — agent resumes in the same worktree with its prior
  commits intact
- Layout is forward-compatible with multi-repo workspaces (add more subdirs under
  `TASK-XXXX/`)

**Cleanup is deferred, not immediate.** Task worktrees are not deleted when a task
transitions to a terminal state — the human may correct an accidental transition.
A periodic cleanup pass removes a worktree only when ALL three conditions hold:

1. Task is in a terminal state (`done`, `failed`, `cancelled`)
2. Task has been in that state continuously for ≥72 hours
3. Worktree has no unpushed commits (`git log @{u}..HEAD` is empty)

Condition 3 is the core safety invariant: never destroy work that has not reached the
remote.

---

## Agent session logs

At task completion, the agent writes a report to:

```
~/.hub/agent-session-logs/TASK-XXXX-<slug>.md
```

Sections: Summary, Changes Made, PRs Created, Gotchas, Next Steps.

The agent registers the path in the task's `links` field:

```bash
hub task link TASK-XXXX ~/.hub/agent-session-logs/TASK-XXXX-<slug>.md
```

The TUI distinguishes file paths from URLs by content (presence of `://`). The `links`
field stores both heterogeneously — no separate column needed. The `agent-session-logs/`
directory is the primary corpus for mining agent pain points and prompt improvement
opportunities.

---

## Prompt files

Kind-specific workflow instructions live in `prompts/`:

| File                        | Kind        | Purpose                                                          |
| --------------------------- | ----------- | ---------------------------------------------------------------- |
| `prompts/implement-task.md` | `implement` | Full implement workflow: read → plan → code → test → commit → PR |
| `prompts/review-task.md`    | `review`    | Review workflow: load diff → assess → comment                    |
| `prompts/debug-task.md`     | `debug`     | Debug workflow: reproduce → isolate → fix → verify               |

These parallel the existing investigate prompts (`ci-investigate.md`,
`issue-investigate.md`, `implement-issue.md`). The `implement-task.md` adapts
`implement-issue.md`, replacing GitHub label transitions with `hub task report` calls
and removing worktree setup (the dispatch workflow handles that).

---

## Cap and timing

- **Cap**: 1 concurrent in-progress session (hardcoded; configurable via hub.toml is
  out of scope for the initial implementation)
- **Dispatch tick**: 30 seconds
- **Session file poll interval**: 10 seconds (reuses existing stream_interval)
- **Idle-to-in-review grace period**: 30 seconds of `status: "idle"` with no report
- **Stall threshold**: 15 minutes of `status: "busy"` with no `updatedAt` advance
- **Window reap buffer**: 5 minutes after `in-review` transition before killing tmux window

---

## Two session launch patterns

The TUI launches two kinds of Claude Code sessions and they share only the `-e` env-var
injection technique. An agent implementing task dispatch must NOT copy the investigation
pattern — the tmux commands, window lifetime, and completion signals differ fundamentally.

|                   | Investigation (`i` key)               | Task dispatch (queue)                                         |
| ----------------- | ------------------------------------- | ------------------------------------------------------------- |
| Triggered by      | Human key press on signal item        | TUI 30s dispatch tick                                         |
| tmux command      | `split-window -h` (attached, in-pane) | `new-window -d -n TASK-XXXX` (detached, named)                |
| Session lifetime  | Ephemeral — human-controlled          | Persistent until `in-review` + 5min reap buffer               |
| Worktree          | Ephemeral or PR-specific              | Persistent `agent/TASK-XXXX` branch                           |
| Completion signal | Human closes the pane                 | `hub task report` (primary) + session file polling (fallback) |
| Session ID        | Not stored                            | Stored in `tasks.session_id` (uuid5, injected at dispatch)    |
| Code lives in     | `ui/tui/src/investigations/`          | `workflows/src/tasks.rs` + `ui/tui/src/main.rs` dispatch tick |

---

## Session file signal reference

The TUI infers agent progress from `~/.claude/sessions/<pid>.json`, which Claude Code
writes and updates for every running process. Fields used:

```
{
  "pid":       42899,
  "sessionId": "<uuid5 injected at dispatch>",
  "cwd":       "<worktree path>",
  "status":    "busy" | "idle",
  "updatedAt": <epoch-ms, changes on every status transition>
}
```

**Signal → transition rules:**

```
Session file state              Condition                  Task transition
─────────────────────────────────────────────────────────────────────────────────
present, status="busy"          updatedAt advancing        → stay in-progress
present, status="busy"          updatedAt stale >15 min    → blocked
present, status="idle"          <30s since idle            → (wait — may be transient)
present, status="idle"          >30s AND no in-review yet  → in-review  [FALLBACK]
absent                          no in-review in DB yet     → in-review  [CRASH RECOVERY]
was blocked, status="busy"      updatedAt advancing        → in-progress [SELF-HEAL]
```

**How to find the session file**: scan `~/.claude/sessions/*.json` for a record whose `sessionId`
matches the dispatched task's `tasks.session_id`. The PID is unknown at dispatch time; the
UUID is the only stable identifier.

**JSONL path**: `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl` where `encoded-cwd` is the
worktree absolute path with `/` replaced by `-`. The last event of every completed turn is
`{"type": "system", "subtype": "turn_duration"}`.

**Risk**: `~/.claude/sessions/` is undocumented. All polling is best-effort; the primary signal
(`hub task report` → DB) is independent. If session file polling fails silently, tasks stay
`in-progress` until manual correction.
