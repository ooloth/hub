# Agent Task Management

Research notes on patterns for dispatching Claude Code agents on tasks, tracking their work,
and surfacing results in the TUI. Informed by studying a mature agent task loop in a work project.

Related issue: #45 (TUI as control plane: dispatch, monitor, and review autonomous agent jobs)

---

## Status Machine

Seven statuses: `backlog → ready → in-progress → blocked/review → done/archived/deleted`

- `ready` is the dispatch signal — a polling loop picks up `status = 'ready'` tasks (oldest first)
- `archived` and `deleted` are soft-delete; records are never hard-deleted
- `in-progress` = agent has claimed the task (has `agent_session_id` set)
- `review` = agent finished, human needs to look

## Atomic Claim

Transition `ready → in-progress` must be atomic:
1. Verify `status = 'ready'` — fail if not (prevents double-claim)
2. Set `status = 'in-progress'`
3. Set `agent_session_id = <uuid>`
4. Write activity log entry
5. Return task details

Active agent count = `SELECT COUNT(*) FROM tasks WHERE status = 'in-progress' AND agent_session_id IS NOT NULL` — simple semaphore for capacity management (e.g. max 2 sessions).

## Deterministic Session IDs

Session ID = `uuid5(namespace, task_key)`. Same task always produces the same ID, so `claude --resume <session_id>` works without a lookup table, and re-dispatching a task doesn't create a new session.

## Task Schema (SQLite)

```sql
tasks: id, task_key ("TASK-0042"), title, description, status, type, parent_id,
       jira_tickets, pr_links, doc_links, agent_session_id, created_at, updated_at

task_comments: id, task_id, author ("human"|"agent"), content, created_at

task_activity: id, task_id, actor, action, detail (JSON), created_at
  -- actions: "created", "status_change", "updated", "comment"
  -- detail: {"from": "ready", "to": "in-progress"}
```

`jira_tickets`, `pr_links`, `doc_links` are comma-separated strings, deduplicated on write.
Small enough that join tables would be overkill.

`task_activity` is immutable — every state change logged with actor, action, JSON detail. Full audit trail.

## Task Types

`implement | debug | general` — type guides which CLAUDE.md skill the agent invokes at dispatch time.
Could map to investigation types in hub (e.g. `ci-failure` auto-loads the Codefresh skill).

## Agent Log Files

Agent creates a markdown log per task (`docs/agent/task-logs/TASK-0042-log.md`), linked back via `doc_links`. Human-readable record of what the agent did and why.

## Claude Code Session Data

Claude Code writes two useful files per session:

- `.claude/session-stats/<session_id>.json` — cost (USD), duration, context % (written by statusline hook)
- `.claude/projects/<project>/<session_id>.jsonl` — every message/tool call (Edit, Bash, Write, Read)

The JSONL can be parsed to show a live agent activity feed in the TUI task detail view. If the stats file is missing, show nothing — don't fail.

## Structured CLI Output

Agent-facing commands emit single-line structured results for machine parsing:

```
TASK_CREATED TASK-0042
TASK_UPDATED TASK-0042
DISPATCHED TASK-0042 <session-id>
RESUMED TASK-0042 <session-id>      ← re-used existing session_id, not a new one
NO_READY_TASKS
AT_CAPACITY
```

Errors → stderr, exit code 1: `ERROR: task TASK-0042 status is 'in-progress', expected 'ready'`

## Dispatch Loop

```
1. SELECT tasks WHERE status='ready' ORDER BY created_at ASC LIMIT 1
2. Check active session count — emit AT_CAPACITY and stop if at limit
3. Claim atomically
4. claude --session-id <deterministic-id> /do-task TASK-0042
5. Log DISPATCHED (or RESUMED if session already existed)
```

Safe to run on a timer — the atomic claim prevents double-dispatch.

## Human-Agent Collaboration

- Human creates task, moves to `ready`, reviews when agent sets `review`
- Both sides write to `task_comments` — agent can read comments via `hub tasks get TASK-X` mid-task
- `task_activity` is the shared audit trail

## TUI Task Detail View

What to surface:
- Metadata (title, description, status, type, timestamps)
- Linked resources (tickets, PR URLs, doc paths) as navigable items
- Comment thread (human + agent, chronological)
- Activity log (immutable)
- Session stats (cost, duration, context %) — from session stats JSON
- Agent activity feed — parsed from session JSONL (tool calls, file edits, commands)

Key actions:
- Create task
- Move to `ready` → triggers dispatch check
- View linked PR → browser
- Resume agent → `claude --resume <agent_session_id>` in new pane

## Schema Migrations

Numbered SQL files (`001_create_tasks.sql`, `002_add_agent_session_id.sql`, ...) applied on
connection, tracked in a `schema_version` table. Auto-applied — no manual migrate step.
WAL mode + foreign keys enabled. Hub already does this; same pattern applies.

## Testing CLI Tools

- Use a temp SQLite DB path per test — no mocking of internal logic
- Test both success and error paths (including precondition failures)
- Test deduplication (adding duplicate ticket → same state)
- `CliRunner` equivalent in Rust: invoke the command handler directly with a test DB path
