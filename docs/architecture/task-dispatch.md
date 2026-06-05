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
HUMAN                          TUI DISPATCH TICK (30s)              AGENT (claude)
  │                                     │                               │
  │ create task                         │                               │
  ├────────────────────────────────────►│                               │
  │                               [backlog]                             │
  │                                     │                               │
  │ s→ready (promote)                   │                               │
  ├────────────────────────────────────►│                               │
  │                               [ready]                               │
  │                                     │                               │
  │                    in_progress count < cap (1)?                     │
  │                    oldest ready task exists?                        │
  │                    → atomic claim: status + session_id              │
  │                               [in-progress] ──────────────────────►│
  │                                     │         tmux new-window -d   │
  │                                     │         -n TASK-XXXX         │
  │                                     │         claude               │
  │                                     │           --session-id uuid  │
  │                                     │           --append-system-   │
  │                                     │             prompt <kind>.md │
  │                                     │           "TASK-XXXX: ..."   │
  │                                     │                               │
  │       ┌─────────────────────────────┤ 10s JSONL poll               │
  │       │ JSONL mtime stale >5m       │                               │
  │       │ + window still exists       │                               │
  │       ▼                             │                               │
  │  [blocked] ─────────────────────────┤ new JSONL activity            │
  │  (High urgency,                     │ → self-heal                   │
  │   needs attention)                  │                               │
  │                                     │ tmux window gone?             │ session exits
  │                                     │ → fallback to in-review       │──►│ hub task
  │                                     │                               │   │   update
  │                               [in-review] ◄──────────────────────────◄──┘ --status
  │                                     │                                   in-review
  │ s→done / s→failed                   │
  ├────────────────────────────────────►│
  │                          [done / failed / cancelled]
```

---

## Component interactions

```
 ┌──────────────────────────────────────────────────────────────────────┐
 │  TUI main loop (tokio::select!)                                       │
 │                                                                       │
 │  30s dispatch_interval ──► workflows::tasks::dispatch()               │
 │      └─ count in-progress tasks in SQLite (cap = 1)                  │
 │      └─ if under cap: fetch oldest ready task                        │
 │          └─ generate uuid5(TASK_NS, task_key)                        │
 │          └─ atomic SQL: claim task (status + session_id, one tx)     │
 │          └─ build prompt: read prompts/<kind>-task.md                │
 │          └─ tmux new-window -d -n TASK-XXXX                          │
 │                 -e HUB_SYSTEM_PROMPT=<kind-prompt>                   │
 │                 -e HUB_TASK_PROMPT=<title+desc+links+comments>       │
 │                 "claude --session-id <uuid>                           │
 │                         --dangerously-skip-permissions               │
 │                         --model opus                                 │
 │                         --append-system-prompt \"$HUB_SYSTEM_PROMPT\"│
 │                         \"$HUB_TASK_PROMPT\""                        │
 │                                                                       │
 │  10s stream_interval ──► poll ALL in-progress tasks                  │
 │      └─ for each: read JSONL mtime                                   │
 │          ├─ mtime stale >5m + window exists → blocked (auto)        │
 │          ├─ new JSONL events + was blocked → in-progress (heal)      │
 │          └─ window gone → in-review (fallback completion)            │
 │      └─ selected task → update stream_blocks (existing behavior)     │
 │                                                                       │
 └──────────────────────────────────────────────────────────────────────┘
         ▲                                           │
         │                                           ▼
 ┌───────────────┐              ┌──────────────────────────────────────┐
 │  hub CLI      │              │  tmux window: TASK-XXXX               │
 │               │              │  claude --session-id <uuid>           │
 │  hub task     │◄─────────────│    --dangerously-skip-permissions    │
 │    update     │  (agent      │    --append-system-prompt <kind>.md  │
 │  hub task     │   calls CLI) │    "TASK-XXXX: title..."             │
 │    comment    │              │                                      │
 │               │              │  writes JSONL to:                    │
 └───────────────┘              │  ~/.claude/projects/                 │
         │                      │    <encoded-cwd>/                    │
         ▼                      │    <session-id>.jsonl               │
 ┌───────────────┐              └──────────────────────────────────────┘
 │  SQLite       │                              │
 │  tasks table  │◄─────────────────────────────┘
 │               │  (TUI polls mtime and checks window existence)
 │  session_id   │
 │  status       │
 └───────────────┘
```

---

## Prompt injection

The dispatch command follows the existing investigation pattern in
`ui/tui/src/investigations/mod.rs`:

- `--append-system-prompt "$HUB_SYSTEM_PROMPT"` — the content of
  `prompts/<kind>-task.md`, injected as a system prompt addition. This carries the
  kind-specific workflow instructions (how to approach an implement/debug/review task).

- `"$HUB_TASK_PROMPT"` — the positional `prompt` argument to `claude`. This is the
  first user message and carries task-specific context: title, description, links,
  agent comment thread, and the done-when instruction (`hub task update TASK-XXXX
  --status in-review`).

Both are passed as tmux `-e` environment variables to avoid OS argument-size limits
and shell quoting complexity.

---

## Completion detection

Primary signal (fast, explicit):
- Agent calls `hub task update TASK-XXXX --status in-review`

Fallback signal (robust):
- TUI polls `tmux list-windows` for the window named `TASK-XXXX`
- Window gone → process exited → transition task to `in-review`
- This handles clean exits where the agent didn't call the CLI

**Load-bearing tmux setting**: `remain-on-exit off` (the default) must stay off.
If windows remain after process exit, the fallback signal never fires and tasks
stay stuck in `in-progress`. See the comment in `~/.config/tmux/tmux.conf`.

---

## Stall detection

When an interactive session blocks at a permission prompt or enters an unexpected
loop, the JSONL file stops growing while the tmux window persists. Hub detects this:

- JSONL mtime stale >5 minutes AND tmux window still exists → auto-transition to
  `blocked` (High urgency, visible in unified list)
- New JSONL activity detected → auto-transition back to `in-progress` (self-heal)

This eliminates the silent-failure mode where an agent stops making progress and the
human has no indication. The human can switch to the `TASK-XXXX` tmux window to
investigate or approve a permission prompt.

`--dangerously-skip-permissions` minimizes permission stalls — most file and shell
operations are auto-approved for task sessions, matching the investigation convention.

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
hub task update TASK-XXXX --link ~/.hub/agent-session-logs/TASK-XXXX-<slug>.md
```

The TUI distinguishes file paths from URLs by content (presence of `://`). The `links`
field stores both heterogeneously — no separate column needed. The `agent-session-logs/`
directory is the primary corpus for mining agent pain points and prompt improvement
opportunities.

---

## Prompt files

Kind-specific workflow instructions live in `prompts/`:

| File | Kind | Purpose |
|---|---|---|
| `prompts/implement-task.md` | `implement` | Full implement workflow: read → plan → code → test → commit → PR |
| `prompts/review-task.md` | `review` | Review workflow: load diff → assess → comment |
| `prompts/debug-task.md` | `debug` | Debug workflow: reproduce → isolate → fix → verify |

These parallel the existing investigate prompts (`ci-investigate.md`,
`issue-investigate.md`, `implement-issue.md`). The `implement-task.md` adapts
`implement-issue.md`, replacing GitHub label transitions with `hub task update` calls
and removing worktree setup (the dispatch workflow handles that).

---

## Cap and timing

- **Cap**: 1 concurrent in-progress session (hardcoded; configurable via hub.toml is
  out of scope for the initial implementation)
- **Dispatch tick**: 30 seconds
- **JSONL poll interval**: 10 seconds (reuses existing stream_interval)
- **Stall threshold**: 5 minutes of JSONL inactivity with window still alive
