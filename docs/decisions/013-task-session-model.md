# 013 — One task, one session; comments as agent log; JSONL-driven status

> For the system as built see [architecture/tasks.md](../architecture/tasks.md)
> (model) and [architecture/task-dispatch.md](../architecture/task-dispatch.md)
> (mechanics); for the why see [vision.md](../vision.md). This ADR records the
> original decision and rationale.

## Context

[Decision 012](012-task-model.md) established the single `AgentTask` type and the
outcome-oriented lifecycle. It did not resolve four questions that only surfaced
during implementation of the TUI task management surface (issue #268):

1. **Can a task have multiple agent attempts?** 012 described re-dispatching rejected
   tasks back to `ready` with human feedback — implying many agents could work the
   same task sequentially.

2. **Who writes task comments?** 012 described `task_comments` as a shared channel:
   "both the human and the agent write to it; the agent reads the full comment thread
   on every resume to pick up human feedback."

3. **How does hub know whether a session is active or complete without trusting agent
   obedience?** 012 was silent on this; the implicit assumption was that agents would
   reliably call `hub task report TASK-XXXX --status in-review` when done.

4. **What are the terminal states and what do they mean?** 012 had `done` and
   `archived` (since renamed `cancelled`). A third terminal state — tasks where the
   agent attempted the work but the result was not accepted — was unaddressed.

## Decisions

### One task, one session

Each task is linked to at most one agent session. There is no re-dispatch of the same
task to a new agent. If an agent attempt produces an unacceptable result, the task is
closed as `failed` and a new task is created with improved inputs (better description,
better skill prompt).

**Why:** Agent sessions are stateful and contextual. A new agent dispatched on the
"same" task starts without the prior agent's reasoning, attempts, and discoveries.
Directing it via feedback comments creates false continuity — it is effectively
starting fresh. The right improvement loop is to fix the *inputs* (description, skill)
and run a new task, not to pile instructions onto a stale attempt.

**Consequences:**
- No `in-review → ready` transition exists.
- Retry is always: close the original task (`failed` or `cancelled`) + create a new
  task with improved inputs.
- `session_id` is assigned once at dispatch and never changes.

### Comments are agent → human only

Task comments are written exclusively by agents. They serve as a captain's log:
proactive explanations of choices made, friction encountered, and trade-offs taken.
They are not a dialogue surface.

**Why:** If a human wants to discuss the session, ask questions, or request changes,
the right mechanism is to resume the session interactively in a new tmux window. The
session has full context of what happened; a comment thread does not. Bidirectional
comments imply dialogue without delivering it.

**Consequences:**
- The TUI has no "add comment" action.
- The `author` field in `task_comments` is always `'agent'` in practice. The schema
  stays permissive (`CHECK(author IN ('human', 'agent'))`) for tooling flexibility,
  but no TUI path writes human comments.
- `o` (open session) is the human's mechanism for interactive dialogue.

### Deterministic session ID via uuid5, passed at launch

At dispatch time, hub computes `session_id = uuid5(NAMESPACE, task_key)` and passes
it to Claude Code via `--session-id <uuid>`. This value is stored in the `tasks` row
atomically with the dispatch claim (`status = 'in-progress'`).

**Why:** Hub controls the session ID rather than receiving one from Claude Code after
the fact. If the DB write fails after launching Claude Code, the session ID is
recoverable from the task key alone — the session is not lost. The agent does not
need to know or store the session ID.

The dispatch transaction is: compute uuid5 → begin DB tx → claim task →
write `session_id` → commit → launch Claude Code with `--session-id <uuid>`.
No partial state is possible.

### JSONL polling as truth source for session liveness

**Superseded (this section only):** The JSONL-based completion model below was
invalidated by a lifecycle spike: interactive sessions never emit `result` events,
and JSONL mtime polling was replaced by `~/.claude/sessions/<pid>.json` status field
polling. The correct model is in [Decision 014](014-task-dispatch.md) and
[docs/architecture/task-dispatch.md](../architecture/task-dispatch.md). The rest of
this decision (session model, comments, UUID5, terminal states) remains valid.

~~The TUI derives session state by polling the session's JSONL file alongside the task
DB:~~

- ~~JSONL file mtime recently changed → session is active (in-progress)~~
- ~~`{"type": "result", ...}` event present in JSONL → session completed → treat as
  in-review, regardless of DB status~~

Agent CLI calls (`hub task report TASK-XXXX --status in-review`) remain the primary
completion signal — they are fast and explicit. Session file polling is the robust
fallback: if the agent crashes, fails to run the CLI, or the human resumes the session
interactively, the TUI still reflects reality.

**Why:** Depending solely on agent obedience creates a class of bugs where tasks stay
stuck in `in-progress` forever if a session exits uncleanly.

**Consequences:**
- When the human presses `o` to resume a session in tmux, the session file transitions
  `status: "idle" → "busy"` → TUI auto-transitions `in-review → in-progress`.
- When the resumed session's turn ends, `status: "idle"` + no new activity → TUI
  auto-transitions back to `in-review` after the grace period.
- Manual status changes via the `s` submenu remain available as an escape hatch.

### Three terminal states: done, failed, cancelled

| Status | Meaning | Reachable from |
|---|---|---|
| `done` | Completed to satisfaction | `in-review` |
| `failed` | Agent attempted; result not accepted | `in-review` |
| `cancelled` | Abandoned at any stage for any reason | `backlog`, `ready`, `in-progress`, `in-review` |

`failed` is not `cancelled`. `cancelled` means "I've decided not to pursue this" —
the idea itself is being set aside, before or after dispatch. `failed` means "we
tried, the output wasn't good enough" — the idea may be retried with a new task.

`cancelled` is available from `in-review` because the human may realise mid-review
that the idea itself was wrong — not that the agent failed, but that the work
shouldn't have been done at all.

The distinction is low-cost (one additional status value) and informative for later
reflection: `session_id IS NOT NULL AND status = 'done'` means the delegation
succeeded; `session_id IS NOT NULL AND status = 'failed'` means it did not.

Hub is not the quality gate. GitHub PR review and merge is the real quality signal.
Hub's `done` means "I've acknowledged and closed this task," not "I certify the
output is correct."

## Consequences for Decision 012

- The bidirectional comment model in 012 ("the agent reads the full comment thread on
  every resume to pick up human feedback") is superseded. Comments are agent-authored
  only.
- The `in-review → ready` rejection path in 012's human lifecycle is removed.
- `archived` was already renamed to `cancelled` during S2/S3 implementation.
- `review` was already renamed to `in-review` during S2/S3 implementation.
- `failed` is a new terminal status added by this decision.
