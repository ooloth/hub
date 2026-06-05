# Tasks

Tasks are the unit of delegated work in hub. A task is a desired outcome assigned to
an agent: "fix this CI failure", "implement this feature", "investigate this alert".
The agent pursues the outcome until done or blocked, then signals for human review.

See [Decision 012](../decisions/012-task-model.md) and
[Decision 013](../decisions/013-task-session-model.md) for the reasoning behind this
model.

---

## One task, one session

Each task is linked to at most one agent session. The `session_id` field is `None`
before dispatch and `Some` once an agent has claimed the task — it never changes
after that.

If an agent attempt produces an unacceptable result, the task is closed (`failed` or
`cancelled`) and a new task is created with improved inputs (better description, better
skill prompt). There is no re-dispatch of the same task to a new agent.

---

## Lifecycle

```
backlog → ready → in-progress → in-review → done
                                           → failed
any status → cancelled
```

| Status | Meaning | `session_id` |
|---|---|---|
| `backlog` | Drafted; not yet committed to the queue | None |
| `ready` | Committed; eligible for agent dispatch | None |
| `in-progress` | Agent has claimed it and is working | Some |
| `in-review` | Session completed; human should look | Some |
| `done` | Closed — completed to satisfaction | Some |
| `failed` | Closed — agent attempted; result not accepted | Some |
| `cancelled` | Closed — abandoned at any stage for any reason | Any |

`cancelled` is distinct from `failed`. `cancelled` means the idea is being set aside —
before or after dispatch. `failed` means the agent did the work and the output wasn't
good enough. Either may follow from `in-review`; `cancelled` is also available from
`backlog`, `ready`, and `in-progress`.

`done` and `in-review` tasks are visible in the unified list for 7 days after
transition so accidental closures can be reversed via the `s` submenu. `failed` and
`cancelled` follow the same rule.

---

## Schema

```sql
CREATE TABLE tasks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT    NOT NULL,
    description TEXT,
    status      TEXT    NOT NULL DEFAULT 'backlog'
                CHECK(status IN (
                    'backlog','ready','in-progress','in-review',
                    'done','failed','cancelled'
                )),
    kind        TEXT    NOT NULL DEFAULT 'general'
                CHECK(kind IN ('implement','debug','general')),
    session_id  TEXT,
    issue_links TEXT,   -- comma-separated URLs
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

`description`, `issue_links`, `pr_links`, and `doc_links` are nullable. Link fields
are comma-separated strings, deduplicated on write.

`task_comments` is an agent-authored captain's log. The agent records choices made,
friction encountered, and trade-offs taken so the human has context when reviewing.
The schema accepts `'human'` as an author for tooling flexibility, but no TUI path
writes human comments. Human-agent dialogue happens by resuming the session
interactively (see `o` key below).

---

## Skill routing via `kind`

The `kind` field routes a task to the appropriate skill at dispatch:

| `kind` | Skill invoked |
|---|---|
| `implement` | Implementation workflow (read → write → test → commit → PR) |
| `debug` | Debug workflow (investigate → identify → fix → verify) |
| `general` | General purpose (agent decides approach from task description) |

Skills encode *how* to do a type of work. Tasks carry *what* to do.

---

## Lifecycle from each side

### Human (TUI)

1. Create a task from scratch or from a signal row (CI failure, GitHub issue, etc.)
   → lands in `backlog`
2. Promote to `ready` via the `s` submenu — makes the task eligible for dispatch
3. Monitor progress in the unified list as the agent works
4. When status reaches `in-review`, read the agent's comment thread for context
5. Press `o` at any time on a task with a `session_id` to open the session in a new
   tmux window — useful for asking questions or requesting changes interactively
6. Close the task via the `s` submenu:
   - `done` — completed to satisfaction (from `in-review`)
   - `failed` — agent attempted; result not accepted (from `in-review`)
   - `cancelled` — abandoning the work at any stage (from any status)
7. If retry is warranted: create a new task with an improved description and/or
   updated skill prompt

### Agent (CLI)

The agent is spawned by the TUI — it does not dispatch itself. The commands below
are the toolkit handed to an already-running agent session.

1. `hub task get TASK-XXXX` — reads the full task as JSON: title, description, kind,
   status, linked resources, comments in chronological order. Called at session start.
2. `hub task comment TASK-XXXX --content "..."` — appends a captain's log entry:
   choices made, friction, trade-offs, anything the human might wonder about when
   reviewing.
3. `hub task report TASK-XXXX --status in-review|blocked` — reports completion or
   blockage. These are the only status transitions the agent owns; done, failed, and
   cancelled are human decisions made in the TUI.

All CLI output is single-line and machine-parseable. Errors go to stderr, exit 1,
with an actionable message.

---

## TUI rendering

Tasks appear in the unified urgency list alongside PRs, CI failures, and other
signals:

| Status | Urgency | Rationale |
|---|---|---|
| `in-review`, `blocked` | High (orange) | Human action required |
| `in-progress` | Low | Monitoring only |
| `backlog`, `ready` | Low | Queued, not demanding attention |

The detail pane (opened with Enter) renders based on `session_id`:

**No session** (`backlog`, `ready`):
- Task metadata: title, description, kind, status, linked resources

**Session exists** (`in-progress`, `in-review`):
- Everything above, plus:
- Agent comment thread (read-only)
- Live JSONL stream (bottom-left pane, polled every 10s)
- Session metrics: cost, context %, elapsed time, turn count

**Terminal** (`done`, `failed`, `cancelled`) — visible for 7 days:
- Task metadata and agent comment thread (read-only)
- Historical JSONL stream and final session metrics

---

## Atomic claim and deterministic session IDs

Dispatch is safe to run on a timer or by multiple concurrent callers because the
claim is atomic: `status = 'ready'` is verified and `status = 'in-progress' +
session_id` are set in one transaction. A concurrent caller racing the same task
gets an error.

Session IDs are deterministic: `uuid5(NAMESPACE, task_key)`. Hub computes this value
before dispatching and passes it to Claude Code via `--session-id <uuid>`, then
stores it in the DB atomically with the dispatch claim. If the DB write fails after
launching Claude Code, the session ID is recoverable from the task key alone — no
session is ever lost.

---

## Session file-driven status inference

The TUI does not depend solely on agent CLI calls to know session state. It polls
`~/.claude/sessions/<pid>.json` — a file Claude Code writes for every running process —
using the task's stored `session_id` to find the matching file:

- **`status: "busy"`, `updatedAt` advancing** → session is active; ensure task is `in-progress`
- **`status: "idle"` for >30s, no `in-review` in DB** → turn complete; transition to `in-review`
- **File absent, no `in-review` in DB** → process exited; transition to `in-review` (crash recovery)
- **`status: "busy"`, `updatedAt` stale >15 min** → session stalled; transition to `blocked`
- **`status: "busy"` with fresh `updatedAt`, was `blocked`** → self-heal to `in-progress`

This makes status tracking robust to crashes and unclean exits. It also handles the
interactive resume case: when the human presses `o` to resume a session in tmux, the
session file transitions `status: "idle" → "busy"` and the TUI automatically transitions
the task from `in-review` back to `in-progress`.

Manual status changes via the `s` submenu remain available as an escape hatch.

`~/.claude/sessions/` is an undocumented internal API. If polling fails, tasks stay
`in-progress` until manual correction. The primary signal (`hub task report` CLI call)
is independent and always works.

See [task-dispatch.md](task-dispatch.md) for the full session file signal reference.

---

## Session resume (`o` key)

On any task where `session_id` is set (in-progress, in-review, done, failed), `o`
opens a new tmux window running:

```
claude --resume <session_id>
```

This is the human's surface for interactive dialogue with the agent: asking why it
made a choice, requesting a specific change, exploring an alternative approach. The
session has full context of what happened; a comment thread does not.

Status transitions after resume follow from JSONL polling automatically — no manual
status change is needed.

---

## Recurring tasks (future)

Tasks can be scheduled on a cadence rather than promoted manually. A recurring task
is created on a schedule, promoted to `ready` automatically, and claimed by the agent
polling loop like any other task.

The scheduling mechanism (cron expression, cadence field, or a separate table) is
deferred to implementation.

---

## What is not yet built

### Dispatch pipeline (see [task-dispatch.md](task-dispatch.md) and [Decision 014](../decisions/014-task-dispatch.md))

- **S0** — Task workspace infrastructure: `ensure_task_worktree()` creates
  `~/.hub/workspaces/TASK-XXXX/<project>/` on branch `agent/TASK-XXXX`; deferred
  periodic cleanup (72h + no unpushed commits guard)
- **S1** — Dispatch loop: 30s TUI tick, atomic `claim_for_dispatch()` SQL transaction,
  uuid5 session ID, `tmux new-window -d -n TASK-XXXX` claude spawn
- **S2** — Prompt surface: `prompts/implement-task.md`, `review-task.md`, `debug-task.md`;
  `HUB_TASK_PROMPT` built from task fields; `hub task link TASK-XXXX <value>` CLI
  command (new subcommand so agents can register session log file paths)
- **S3** — Completion + stall detection: session file `status: "idle"` >30s → `in-review`
  fallback; file absent → `in-review` (crash); `updatedAt` stale >15m → `blocked`;
  fresh `updatedAt` → self-heal to `in-progress`; 5-min window reap on `in-review`

### TUI task surface (from issue #268, now closed)

- TUI task creation modal: `n` opens multi-field form
- TUI task creation from signal row: pre-populated form
- TUI detail pane for `backlog`/`ready` tasks
- TUI agent comment thread (read-only) in detail pane
- TUI `o` key: open session in tmux (`claude --resume <session_id>`)
- TUI `done`/`failed`/`cancelled` via `s` submenu from `in-review`
- TUI `cancelled` via `s` submenu from `in-progress`

### Infrastructure

- Session file polling extended to all in-progress tasks (currently selection-scoped only; `stream_interval`)
- Database migration: move `hub.db` from `~/Library/Application Support/hub/` to
  `~/.hub/hub.db` (separate issue; not blocking dispatch)
