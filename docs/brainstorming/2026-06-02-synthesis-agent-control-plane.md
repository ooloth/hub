# Synthesis: Agent Control Plane Roadmap

Distilled from studying a mature agent task loop in a work project (see
`2026-06-02-inspiration-personal-control-center.md` and
`2026-06-02-agent-task-management.md`). Records which ideas fit hub and in what order.

Related issue: #45 (TUI as control plane) — may be superseded by this broader scope.

---

## What hub has today

- Urgency-ranked unified list (PRs, Issues, CI, Linear, Loki, GCP)
- Investigation skill routing (keypress → Claude Code skill pre-loaded with context)
- Split detail pane, action modals (review, merge, dismiss)
- Single-row SQLite status cache (30-min TTL)
- Minimal CLI skeleton (no subcommands)
- No task management, no dispatch loop, no agent monitoring

---

## Perfect fits

### Agent system (Part 4 of inspiration docs) — nearly all of it

This section maps directly to what hub is trying to build.

**Task store:**
- `tasks`, `task_comments`, `task_activity` SQLite tables in `store/`
- Numbered SQL migrations tracked in `schema_version` (hub already has this pattern)
- Soft deletes via status field — never hard-delete
- `GENERATED ALWAYS AS` task_key: `TASK-0042` from integer PK, indexed, no app logic
- Comma-separated `jira_tickets`, `pr_links`, `doc_links` — deduplicated on write

**Status machine:**
```
backlog → ready → in-progress → blocked / review → done / archived
```
- `ready` is the dispatch signal (polling picks up oldest-first)
- `in-progress` = agent has claimed the task (`agent_session_id` set)
- `review` = agent finished, human needs to look

**Atomic claim (critical):**
1. Verify `status = 'ready'` — fail if not (prevents double-claim)
2. Set `status = 'in-progress'`, set `agent_session_id`
3. Write activity log entry
4. Return task details

**Dispatch:**
- Deterministic session IDs: `uuid5(namespace, task_key)` — same task → same session → `claude --resume` works without a lookup
- Capacity check: `COUNT WHERE status='in-progress' AND agent_session_id IS NOT NULL`
- `AT_CAPACITY` and `NO_READY_TASKS` exit 0 — expected states, not errors
- Safe to run on a timer

**CLI output codes (machine-parseable):**
```
TASK_CREATED TASK-0042
TASK_UPDATED TASK-0042
DISPATCHED   TASK-0042 <session-id>
RESUMED      TASK-0042 <session-id>   ← re-used existing session, not new
NO_READY_TASKS
AT_CAPACITY
```
Errors → stderr, exit 1: `ERROR: task TASK-0042 status is 'in-progress', expected 'ready'`

**Agent session observability:**
- `.claude/session-stats/<session_id>.json` — cost, duration, context% (written by statusline hook hub already has)
- `.claude/projects/<project>/<session_id>.jsonl` — every message/tool call
- JSONL parsed line-by-line, capped at ~200 messages, system messages filtered
- Three block types: diff (old/new with −/+), bash command, file write
- Context % as colored bar: green <75%, yellow 75–90%, red >90%

**TUI split panel:**
```
No agents running → full-width urgency list
Agents running    → 50/50 split:

┌─ Urgency List ──────────────┬─ Running Agents ──────────┐
│ CRITICAL                    │ ⊛ TASK-0042  ooloth/hub   │
│  • PR#42 · auth refactor    │   Implementing…  18m 42s  │
│ HIGH                        │   Bash · Edit x2 · Read   │
│  • Issue#78 · docs update   │   Cost: $0.14  Ctx: 42%   │
└─────────────────────────────┴───────────────────────────┘
```

**Keybindings:**
- `d` — dispatch modal (model, worktree mode) → ENTER → spawn → show agent detail
- `y` / `n` — approve / reject (when agent sets `review`)
- `v` — view transcript
- `i` — interrupt agent
- `Esc` — back

**Human-agent collaboration:**
- Both sides write to `task_comments`; agent reads comments on resume via CLI
- `task_activity` is the shared immutable audit trail
- Agent creates a markdown log per task, linked back via `doc_links`

**Task type routing:**
- `implement | debug | general` → determines which skill loads at dispatch
- Extends hub's existing investigation routing model

### CLI and DB patterns (Part 1)

- Flexible reference resolution: accept both `42` and `TASK-0042`
- Test pattern: isolated temp SQLite DB per test, real migrations run, test success + precondition-failure paths

### UI/UX gems (Part 2)

- Color map for task statuses: `backlog=indigo ready=blue in-progress=cyan blocked=red review=orange done=green archived=dimmed`
- Section headers with item counts: "Needs review (3)"
- Explicit truncation indicator — never silently hide data

### CI failure + task link (Part 3)

- Once tasks exist: show `TASK-0042` badge on CI failure rows that have a linked task; offer "create task" action when absent

---

## Not a fit (skip or defer)

- SSE event stream, DataSourceManager — hub uses tokio MPSC; no web layer
- File-based TTL cache — hub has SQLite cache already
- Workspace management (`_CANONICAL`, split venv install, `commit-push`) — Python-specific; hub uses git worktrees already
- Todo board / kanban — hub isn't a todo app
- Calendar, notes, repo management screen — out of scope
- Snooze (sidecar JSON) — useful later for PR list, not Phase 1

---

## Execution phases

### Phase 1 — Task store + CLI
1. Add `tasks`, `task_comments`, `task_activity` tables to `store/` with numbered migrations
2. `hub tasks create / list / get / ready / update / comment` CLI commands
3. Structured output codes (`TASK_CREATED`, `AT_CAPACITY`, etc.)
4. Flexible reference resolution (`42` or `TASK-0042`)

### Phase 2 — Dispatch CLI
5. `hub tasks dispatch` — atomic claim, deterministic session ID, capacity check, spawn Claude Code

### Phase 3 — TUI agent control plane ← **biggest game changer; do this next**
6. `AgentSession` domain type (session_id, status, cost_usd, context_pct, turns, activity_feed)
7. Split panel layout: urgency list + running agents side-by-side
8. JSONL parser: line-by-line, filter system messages, three block types
9. Agent detail view: metadata + activity feed + actions footer
10. Keybindings: `d` dispatch modal, `y`/`n` approve/reject, `v` transcript, `i` interrupt

### Phase 4 — CI failure + task link
11. Show `TASK-XXXX` badge on CI failure rows with a linked task
12. "Create task" action on CI failure rows with no linked task

### Phase 5 — Home screen tiles (after task management is proven)
13. KPI summary cards per signal (PR, CI, Linear, Loki, Agent tasks)
14. Two-tier layout: cards top + urgency ranking list bottom-right + agent tasks card bottom-left
15. Per-card urgency thresholds (see status card reference in inspiration doc)

---

## On issue #45

Issue #45 ("TUI as control plane") covers the jobs panel, transcript access, and abort — a subset of Phase 3. This roadmap expands scope significantly (task store, CLI, dispatch loop). Consider updating #45 with the full scope or closing it as superseded when starting implementation.
