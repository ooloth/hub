# Tasks

Tasks are the unit of delegated work in hub. A task is a desired outcome
assigned to an agent: "fix this CI failure", "implement this feature",
"investigate this alert". The agent pursues the outcome until done or
blocked, then signals for human review.

See [Decision 012](../decisions/012-task-model.md) for the reasoning
behind this model.

---

## One type, full lifecycle

There is one domain type — `AgentTask` — that carries a task from idea to
completion. There is no separate "agent session" type. The `session_id`
field is `None` before dispatch and `Some` once an agent has claimed the
task.

```
backlog → ready → in-progress → blocked ─┐
                                          ├→ review → done
                              in-progress ┘
backlog / ready / in-progress / blocked / review → archived
```

| Status | Meaning | `session_id` |
|---|---|---|
| `backlog` | Drafted; not yet committed to the queue | None |
| `ready` | Committed; eligible for agent dispatch | None |
| `in-progress` | Agent has claimed it and is working | Some |
| `blocked` | Agent cannot continue; human attention needed | Some |
| `review` | Agent finished; human must approve or reject | Some |
| `done` | Approved and closed | Some |
| `archived` | Withdrawn without completing | Any |

---

## Schema

```sql
CREATE TABLE tasks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT    NOT NULL,
    description TEXT,
    status      TEXT    NOT NULL DEFAULT 'backlog'
                CHECK(status IN ('backlog','ready','in-progress','blocked','review','done','archived')),
    kind        TEXT    NOT NULL DEFAULT 'general'
                CHECK(kind IN ('implement','debug','general')),
    session_id  TEXT,
    pr_links    TEXT,   -- comma-separated URLs
    doc_links   TEXT,   -- comma-separated relative paths
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL
);

CREATE TABLE task_comments (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author     TEXT    NOT NULL CHECK(author IN ('human', 'agent')),
    content    TEXT    NOT NULL,
    created_at TEXT    NOT NULL
);
```

`description`, `pr_links`, and `doc_links` are nullable — tasks can be
created with just a title and fleshed out later. `pr_links` and `doc_links`
are comma-separated strings; deduplicated on write.

`task_comments` is the shared communication channel. Both the human and
the agent write to it. The agent reads the full comment thread on every
resume to pick up human feedback.

> **Current state:** The `tasks` table in `store/src/tasks.rs` exists but
> is missing `description`, `pr_links`, `doc_links`, and `updated_at`.
> `task_comments` does not yet exist. These are added as part of the task
> management TUI slice.

---

## Skill routing via `kind`

The `kind` field routes a task to the appropriate skill at dispatch:

| `kind` | Skill invoked |
|---|---|
| `implement` | Implementation workflow (read → write → test → commit → PR) |
| `debug` | Debug workflow (investigate → identify → fix → verify) |
| `general` | General purpose (agent decides approach from task description) |

Skills encode *how* to do a type of work. Tasks carry *what* to do.
Adding a new repeatable workflow means writing a skill, not a new `kind`.

---

## Lifecycle from each side

### Human (TUI)

1. Create a task from scratch or from a signal row (CI failure, GitHub
   issue, etc.) → lands in `backlog`
2. Edit the title, description, kind, and linked resources in the detail
   pane
3. Promote to `ready` when committed — this makes it eligible for dispatch
4. Monitor progress in the unified list as the agent works
5. When status reaches `review`: read the agent's comment, then approve
   (`y` → `done`) or reject (`n` → back to `ready` with a comment)

### Agent (CLI)

1. `hub tasks dispatch [--max-sessions N]` — atomically claims the oldest
   `ready` task, sets `session_id`, transitions to `in-progress`, spawns a
   Claude Code session. Prints `DISPATCHED TASK-XXXX <session-id>` or
   `RESUMED TASK-XXXX <session-id>` if the task already has a session.
   Prints `NO_READY_TASKS` or `AT_CAPACITY` (exit 0) for expected idle
   states.
2. `hub tasks get TASK-XXXX` — reads the full task as JSON: title,
   description, kind, status, linked resources, comments in chronological
   order. Called at session start and on every resume.
3. `hub tasks comment TASK-XXXX --author agent --content "..."` — appends
   a progress note or escalation message.
4. `hub tasks update TASK-XXXX --status review` — signals completion or
   blockage; the TUI shows the task as needing human attention.

All CLI output is single-line and machine-parseable. Errors go to stderr,
exit 1, with an actionable message.

---

## TUI rendering

Tasks appear in the unified urgency list alongside PRs, CI failures, and
other signals. Urgency follows the same rules as other items:

| Status | Urgency | Rationale |
|---|---|---|
| `review`, `blocked` | High (orange) | Human action required |
| `in-progress` | Low | Monitoring only |
| `backlog`, `ready` | Low | Queued, not demanding attention |

`backlog` and `ready` tasks are included in the unified list but shown at Low
urgency — they are queued work, not active signals demanding attention. `done`
and `cancelled` tasks are excluded by default.

The detail pane (opened with Enter) renders in two modes based on
`session_id`:

**No session** (`backlog`, `ready`):
- Task metadata: title, description, kind, status
- Linked resources: PR URLs, doc paths
- Comment thread

**Session exists** (`in-progress`, `blocked`, `review`):
- Everything above, plus:
- Live JSONL stream (bottom-left pane, polled every 10s)
- Session metrics: cost, context %, elapsed time, turn count

---

## Atomic claim and deterministic session IDs

Dispatch is safe to run on a timer or by multiple callers because the
claim is atomic: `status = 'ready'` is verified and `status = 'in-progress'
+ session_id` are set in one transaction. A concurrent caller racing the
same task gets an error.

Session IDs are deterministic: `uuid5(NAMESPACE, task_key)`. The same task
always maps to the same session ID, so re-dispatching an interrupted task
resumes the existing Claude Code session via `claude --session-id <uuid>`
rather than starting a new one.

---

## Recurring tasks (future)

Tasks can be scheduled on a cadence rather than promoted manually. A recurring
task is created on a schedule (e.g. every morning), promoted to `ready`
automatically, and claimed by the agent polling loop like any other task.

This replaces manual routine invocations (currently done via Claude Code Desktop
Routines) with a tracked, observable, approvable equivalent. The same system —
same domain type, same agent loop, same TUI monitoring — handles both one-off
and scheduled work. Scheduled tasks appear in the unified list like any other
task and surface in `in-review` when the agent is done.

The scheduling mechanism (cron expression, cadence field on the task, or a
separate `recurring_tasks` table) is deferred to implementation.

---

## What is not yet built

- `description`, `pr_links`, `doc_links`, `updated_at` columns on `tasks`
- `task_comments` table
- TUI task creation flow (from scratch and from signal rows)
- TUI detail pane for `backlog`/`ready` tasks (no stream pane)
- TUI `y`/`n` approve/reject keybindings on `review` tasks
- `hub tasks dispatch`, `get`, `update`, `comment` CLI commands
- Dispatch: atomic claim transaction, uuid5 session ID, Claude Code spawn
- Showing `backlog`/`ready` tasks in the unified list
