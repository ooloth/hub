# Tasks

Tasks are the unit of delegated work in hub — the spine of the flywheel in
[vision.md](../vision.md). A task is a desired outcome assigned to an agent:
"fix this CI failure", "implement this feature", "investigate this alert".
The agent pursues the outcome until done or blocked, then signals for human
review.

**Scope of this document.** This describes the task **model** as built —
what a task is, its lifecycle, schema, and surfaces. For the dispatch
**mechanics** (atomic claim, tmux spawn, session-file polling, stall
detection), see [task-dispatch.md](task-dispatch.md). For the rationale,
see [Decision 012](../decisions/012-task-model.md) and
[Decision 013](../decisions/013-task-session-model.md).

---

## One task, one session

Each task is linked to at most one agent session. The `session_id` field is
`None` before dispatch and `Some` once an agent has claimed the task — it
never changes after that.

If an agent attempt produces an unacceptable result, the task is closed
(`failed` or `cancelled`) and a new task is created with improved inputs
(better description, better prompt). There is no re-dispatch of the same
task to a new agent.

---

## Lifecycle

```
backlog → ready → in-progress → in-review → done
                       │                   → failed
                       └──► blocked ──► in-progress   (resolved / self-heal)
any status → cancelled
```

| Status | Meaning | `session_id` |
|---|---|---|
| `backlog` | Drafted; not yet committed to the queue | None |
| `ready` | Committed; eligible for agent dispatch | None |
| `in-progress` | Agent has claimed it and is working | Some |
| `blocked` | Agent cannot continue; human attention needed | Some |
| `in-review` | Session completed; human should look | Some |
| `done` | Closed — completed to satisfaction | Some |
| `failed` | Closed — agent attempted; result not accepted | Some |
| `cancelled` | Closed — abandoned at any stage for any reason | Any |

`cancelled` is distinct from `failed`. `cancelled` means the idea is being
set aside — before or after dispatch. `failed` means the agent did the work
and the output wasn't good enough. Either may follow from `in-review`;
`cancelled` is available from any status.

`blocked` is reached automatically when a session stalls (see
[task-dispatch.md](task-dispatch.md)) and self-heals back to `in-progress`
when the session resumes activity.

Terminal tasks (`done`, `failed`, `cancelled`) remain in the unified list
for 7 days after transition so an accidental closure can be reversed via the
`s` submenu; non-terminal tasks are always visible.

---

## Schema

```sql
CREATE TABLE tasks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT    NOT NULL,
    description TEXT,
    status      TEXT    NOT NULL DEFAULT 'backlog'
                CHECK(status IN (
                    'backlog','ready','in-progress','blocked',
                    'in-review','done','failed','cancelled'
                )),
    kind        TEXT    NOT NULL DEFAULT 'implement'
                CHECK(kind IN ('review','implement','debug')),
    session_id  TEXT,
    links       TEXT,   -- comma-separated URLs and file paths
    repo        TEXT,   -- e.g. "ooloth/hub"
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    -- populated on create and every change
);

CREATE TABLE task_comments (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author     TEXT    NOT NULL CHECK(author IN ('human', 'agent')),
    content    TEXT    NOT NULL,
    created_at TEXT    NOT NULL
);
```

Linked resources live in a **single `links` field** — a comma-separated mix
of URLs and file paths, deduplicated on write. The TUI distinguishes a file
path from a URL by content (presence of `://`), so no separate columns are
needed. `repo` is the project slug, used to locate the worktree at dispatch.

`task_comments` is an agent-authored captain's log. The agent records
choices made, friction encountered, and trade-offs taken so the human has
context when reviewing. The schema accepts `'human'` for tooling
flexibility, but no TUI path writes human comments — human-agent dialogue is
intended to happen by resuming the session (see *Session resume* below). See
[Decision 013](../decisions/013-task-session-model.md).

> **Direction.** A typed task↔signal origin (replacing freeform reliance on
> `links`) and a verdict field are specified in
> [Decision 016](../decisions/016-tasks-fold-back-into-signals.md) and
> [Decision 017](../decisions/017-verdict-signal-for-feedback-loop.md). Not
> yet built; this section will be updated when they land.

---

## Kind and skill routing

The `kind` field routes a task at dispatch — selecting both the workflow
prompt and the model:

| `kind` | Workflow | Prompt |
|---|---|---|
| `implement` | read → plan → code → test → commit → PR | `prompts/tasks/implement.md` |
| `debug` | reproduce → isolate → fix → verify | `prompts/tasks/debug.md` |
| `review` | load diff → assess → comment | `prompts/tasks/review.md` |

Prompts encode *how* to do a type of work; tasks carry *what* to do. Model
selection per kind and prompt injection are covered in
[task-dispatch.md](task-dispatch.md).

---

## Lifecycle from each side

### Human (TUI)

1. Create a task from scratch (`N`) or seeded from a signal row (`n` on a
   CI failure, GitHub issue, PR, alert, etc.) → lands in `backlog`
2. Promote to `ready` via the `s` submenu — makes it eligible for dispatch
3. Monitor progress in the unified list as the agent works
4. When status reaches `in-review`, read the agent's comment thread for
   context and review the produced artifact (PR/issue) on its native surface
5. Close the task via the `s` submenu:
   - `done` — completed to satisfaction (from `in-review`)
   - `failed` — agent attempted; result not accepted (from `in-review`)
   - `cancelled` — abandoning the work at any stage (from any status)
6. If retry is warranted: create a new task with improved inputs

> Once tasks fold back into their signals
> ([Decision 016](../decisions/016-tasks-fold-back-into-signals.md)), step 5
> becomes automatic for signal-backed tasks — merging the PR closes the
> task. Not yet built.

### Agent (CLI)

The agent is spawned by the TUI — it does not dispatch itself. These
commands are the toolkit handed to an already-running session:

1. `hub task get TASK-XXXX` — reads the full task as JSON: title,
   description, kind, status, links, comments. Called at session start.
2. `hub task comment TASK-XXXX --content "..."` — appends a captain's log
   entry.
3. `hub task link TASK-XXXX --value <url|path>` — registers an artifact
   (PR URL, session-log path) on the task.
4. `hub task report TASK-XXXX --status in-review|blocked` — reports
   completion or blockage. These are the only transitions the agent owns;
   `done`, `failed`, and `cancelled` are human decisions made in the TUI.

All CLI output is single-line and machine-parseable. Errors go to stderr,
exit 1, with an actionable message.

---

## TUI rendering

Tasks appear in the unified urgency list alongside PRs, CI failures, and
other signals:

| Status | Urgency | Rationale |
|---|---|---|
| `in-review`, `blocked` | High (orange) | Human action required |
| `in-progress` | Low | Monitoring only |
| `backlog`, `ready` | Low | Queued, not demanding attention |

The detail pane renders based on `session_id`:

- **No session** (`backlog`, `ready`): task metadata only.
- **Session exists** (`in-progress`, `blocked`, `in-review`): metadata, the
  agent comment thread (read-only), the live JSONL stream (polled every
  10s), and session metrics (cost, context %, elapsed, turns).
- **Terminal** (`done`, `failed`, `cancelled`), visible 7 days: metadata,
  comment thread, and historical stream/metrics.

---

## Status inference

A task's status changes from three sources: human action (the `s` submenu),
the agent's own `hub task report` call, and **automatic inference** from
Claude Code's session files when the agent finishes, crashes, or stalls. The
inference rules and their thresholds are mechanics — see
[task-dispatch.md](task-dispatch.md) for the full signal reference. The `s`
submenu is always available as a manual escape hatch.

---

## Session resume (planned — [issue 289](https://github.com/ooloth/hub/issues/289))

The intended surface for interactive dialogue with an agent is resuming its
session in a new tmux window (`claude --resume <session_id>`), bound to the
`o` key on any task with a `session_id`. This gives the human full session
context — asking why a choice was made, requesting a change, exploring an
alternative — which a one-way comment thread cannot.

When a resumed session becomes active again, the automatic self-heal
(`blocked`/`in-review` → `in-progress`) already follows from session-file
polling. **The `o` keybinding itself is not yet built.**

---

## Not yet built

- **Session resume `o` key** — [issue 289](https://github.com/ooloth/hub/issues/289).
- **Fold-back into signals** — typed origin, auto-transition on signal
  terminal state, badge/dedup
  ([Decision 016](../decisions/016-tasks-fold-back-into-signals.md)).
- **Verdict signal** for the feedback loop
  ([Decision 017](../decisions/017-verdict-signal-for-feedback-loop.md)).
- **Mining / meta task kind** that proposes prompt and doc improvements
  ([Decision 018](../decisions/018-meta-loop-output-as-labeled-issues.md)).
- **Recurring tasks** — created and promoted to `ready` on a cadence rather
  than by hand. Scheduling mechanism deferred.
- **Multi-repo workspaces** — the `~/.hub/workspaces/TASK-XXXX/<project>/`
  layout supports one repo per task today; additional subdirectories are the
  path to multi-repo, with cross-repo coordination deferred.
