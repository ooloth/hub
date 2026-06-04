# 012 — Task model: one type, outcome-oriented, status-driven lifecycle

## Addendum (2026-06-04)

> **⚠ Partially superseded by [Decision 013](013-task-session-model.md).** The
> following assumptions in this document no longer hold:
>
> - **Bidirectional comments**: `task_comments` was described as a shared channel
>   where both human and agent write, and the agent reads human feedback on resume.
>   Comments are now agent-authored only. Human-agent dialogue happens by resuming
>   the session interactively (`o` key in TUI).
> - **Re-dispatch on rejection**: the `in-review → ready` path (reject with
>   feedback, re-queue for a new agent) is removed. One task = one agent session.
>   Retry means closing the task and creating a new one with better inputs.
> - **Terminal states**: `done` and `archived` (now `cancelled`) are joined by
>   `failed` — for tasks where the agent attempted the work but the result was not
>   accepted. `cancelled` is available from any status including `in-progress` and
>   `in-review`.
> - **Status inference**: the TUI derives session liveness from JSONL file polling
>   rather than depending solely on agent CLI calls.

## Context

[Decision 011](011-tui-extends-to-sessions-and-tasks.md) established that
the TUI owns session tracking and task management. It did not define what
a Task is — its shape, its lifecycle, or the boundary between a task and
an agent session.

Two questions needed settling before implementation could proceed:

**One type or two?** The natural language suggests a distinction: a "task"
(work to be done) versus an "agent session" (work being done). This could
mean two domain types — `Task` and `AgentSession` — with a dispatch event
converting one into the other. Or it could mean one type with a status
field that encodes where in the lifecycle the work sits.

**Single action or desired outcome?** A task could represent one atomic
step ("run cargo check") or an open-ended outcome ("fix the auth flow").
These differ in how an agent reasons about completion, when it stops, and
whether the scope is bounded at creation time.

## Decision

### One type, many statuses

Tasks and agent sessions are the same entity at different lifecycle stages.
There is no separate `AgentSession` type. One `AgentTask` struct carries
the work from idea to completion:

```
backlog → ready → in-progress → blocked → review → done → archived
```

- `backlog` — drafted, not yet committed to the queue
- `ready` — committed; eligible for agent dispatch
- `in-progress` — an agent has claimed it; `session_id` is set
- `blocked` — agent cannot continue; human attention needed
- `review` — agent finished; human must approve before closing
- `done` — approved and closed
- `archived` — withdrawn without completing

The `session_id: Option<String>` field is the session anchor. It is `None`
before dispatch and `Some` once an agent claims the task. The TUI uses
this field to decide what to render in the detail pane — the JSONL stream
and cost/context metrics are only shown when `session_id` is `Some`.

A second type would create a conversion event at the dispatch boundary
with no modelling benefit. One type with a nullable `session_id` is
equivalent and simpler.

### Tasks represent desired outcomes

A task is a desired outcome, not a single action. The agent pursues the
outcome — reading code, making changes, running checks, creating PRs —
until it is done or blocked, then signals `review` and stops.

Single-step work is a degenerate case of an outcome (the outcome requires
one step). Modelling tasks as single actions would push orchestration
logic outside the task system, duplicating what skills already provide.

### Skills encode the how; tasks are instances

Reusable multi-step workflows belong in skills (`.claude/skills/`).
The `kind` field (`implement`, `debug`, `general`) routes a task to the
appropriate skill at dispatch time. Adding a new repeatable workflow means
writing a skill, not a new task type.

Tasks carry the *what* (title, description, linked resources, context) and
the *where in the lifecycle* (status). Skills carry the *how*.

### Human-facing surface is the TUI; agent-facing surface is the CLI

Humans create, promote, and review tasks through the TUI. The CLI (`hub
tasks get/update/comment/dispatch`) is the agent-facing protocol — the
machine-parseable surface agents call during their sessions. Humans do
not need to run CLI task commands from a terminal.

## Consequences

- [Decision 010](010-hub-cli-as-agent-toolkit.md) is refined: the specific
  agent-facing commands are `get`, `update`, `comment`, and `dispatch`.
  These are the canonical way agents read their task and signal progress.
- `AgentTask` in `domain/` is the single domain type for the full
  lifecycle. The `StatusItem::AgentSession` variant name in
  `workflows/src/status.rs` is a rendering hint (this item has an active
  session), not a separate domain concept.
- The TUI unified list shows tasks at all visible statuses. `backlog` and
  `ready` tasks appear alongside `in-progress`, `blocked`, and `review`
  items — filtered by status the same way PRs are filtered by draft state.
- The TUI detail pane renders differently based on `session_id`: task
  metadata and comments always; JSONL stream and session metrics only when
  a session exists.
- `done` and `archived` tasks are excluded from the unified list by
  default (same as closed PRs).
- Parent/child task relationships (`parent_id`) are a future escape hatch
  for outcomes too large for one session. Not part of the initial
  implementation.
