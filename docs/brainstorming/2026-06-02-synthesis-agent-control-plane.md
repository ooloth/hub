# Synthesis: Agent Control Plane Roadmap

> **Raw research — superseded by [vision.md](../vision.md).** Kept for
> provenance; not current truth.

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
- Minimal CLI skeleton (`ui/cli/src/main.rs` — clap derive, empty `Cli {}` struct, no subcommands)
- No task management, no dispatch loop, no agent monitoring

**Key files an agent must know before touching anything:**

| File | What it is |
|------|-----------|
| `domain/src/lib.rs` | Pure types only — no I/O. New domain types go here. |
| `workflows/src/status.rs` | `StatusItem` enum + `StatusReport` struct + `SCHEMA_VERSION` constant |
| `store/src/status.rs` | SQLite connect/migrate/read/write pattern to follow for new tables |
| `ui/cli/src/main.rs` | Clap derive skeleton — add subcommands here, not in a new binary |
| `ui/cli/README.md` | Clap derive subcommand patterns to follow |
| `store/README.md` | Rusqlite connection and query patterns to follow |
| `ui/tui/src/state/types.rs` | `Screen` enum and list state — where `AgentSession` items plug in |
| `ui/tui/README.md` | `SCHEMA_VERSION` bump rules — read before touching `StatusReport` |

**Import direction (enforced — never violate):**
```
ui/ → config/
   → workflows/ → clients/ → domain/
                → store/   → domain/
```
`domain/` has no deps on other hub crates. `store/` only touches SQLite. `workflows/` orchestrates. Config values are passed as function args into workflows — workflows do not import `config/` directly.

---

## Perfect fits

### Agent system (Part 4 of inspiration docs) — nearly all of it

This section maps directly to what hub is trying to build.

**Task store:**
- `tasks` and `task_comments` SQLite tables in `store/` — use the same `ensure_table`
  pattern as `store/src/status.rs` (inline SQL, `CREATE TABLE IF NOT EXISTS`)
- `task_activity` (immutable audit log) — deferred; not needed for Phase 1-3
- Numbered SQL migration runner — deferred; `ensure_table` per table is sufficient
  until schema evolution is actually needed
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
3. Return task details

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

**TUI layout — same pattern as existing PR detail view:**

Agent sessions appear as items in the unified urgency list (mixed with PRs, issues, CI).
Selecting one auto-opens the detail panels below — no separate layout mode.

```
┌─ All ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ Agent · TASK-0042 · auth refactor · review · 18m · $0.14 · ctx 42%                                                                                            ooloth/hub · 18m ago │
│ Agent · TASK-0043 · fix CI flakiness · in-progress · 4m · $0.03 · ctx 12%                                                                                     ooloth/hub · 4m ago  │
│ PR · gmail: enforce TLS certificate verification in STARTTLS connection #42 · ooloth · no reviews                                                         ooloth/media-tools · 14d │
│ PR · Add project overview, integrations, and dev commands to CLAUDE.md #83 · ooloth · no reviews · conflict                                          ooloth/michaeluloth.com · 13d │
│ PR · Fix wrong variable name and API in README.invariant.md #84 · ooloth · no reviews                                                                ooloth/michaeluloth.com · 13d │
│ PR · cli: fix broken import and stale docstring #55 · ooloth · no reviews · conflict                                                                          ooloth/scripts · 12d │
│ PR · workflows/implement: filter claude stderr before forwarding to caller #159 · ooloth · no reviews                                                             ooloth/hub · 12d │
│ PR · docs: add cargo-nextest to CONTRIBUTING prerequisites #210 · ooloth · no reviews                                                                              ooloth/hub · 8d │
│ PR · docs: add deploy playbook and fix CLAUDE.md reference #49 · ooloth · no reviews                                                                       ooloth/media-tools · 8d │
│ CI · workflows/build: cargo check failed                                                                                                                       ooloth/hub · 2h ago │
│                                                                                                                                                                                    │
│ Issue · Propagate importer error context in status.rs instead of silently dropping it #12 · author:agent, status:needs-human-review                       ooloth/hub-private · 16d │
│ Issue · Replace let mut accumulation loop in importer.rs with iterator partition #14 · author:agent, status:needs-human-review                            ooloth/hub-private · 16d │
│ Issue · Non-obvious VPN network namespace constraint not documented for agents #7 · author:agent, status:needs-human-review                                     ooloth/media · 16d │
│ Issue · Document observability signals so agents can diagnose failures #66 · author:agent, status:needs-human-review                                              ooloth/hub · 16d │
│ Issue · Add CLAUDE.md to orient agents to project purpose #5 · author:agent, status:needs-human-review                                                 ooloth/advent-of-code · 16d │
│                                                                                                                                                                                    │
│                                                                                                                                                                                    │
│                                                                                                                                                                                    │
│                                                                                                                                                                                    │
│                                                                                                                                                                                    │
├─ session stream ─────────────────────────────────────────────────────────────────────────────────────────────┬─ task info ─────────────────────────────────────────────────────────┤
│ 14:39  E  Edit /src/domain/types.rs                                                                         │ TASK-0042 · auth refactor                                            │
│           - pub fn session_id(&self) -> String {                                                            │                                                                      │
│           + pub fn session_id(&self) -> &str {                                                              │ status     review                                                    │
│ 14:38  +  Bash: cargo test -p hub-tui -- 42 passed (3.2s)                                                   │ type       implement                                                 │
│ 14:37  >  "Running tests to verify the auth changes work correctly..."                                      │                                                                      │
│ 14:36  E  Edit /src/config.rs                                                                               │ pr         ooloth/hub #214                                           │
│           - let session = self.session.clone();                                                             │ ticket     HUB-42                                                    │
│           + let session = &self.session;                                                                    │                                                                      │
│ 14:35  o  Read /docs/architecture/secrets.md (4.2k)                                                         │ cost       $0.14                                                     │
│ 14:34  +  Bash: just check -- passed                                                                        │ ctx        ########..  42%                                           │
│ 14:33  >  "Starting with config to understand auth flow..."                                                 │ elapsed    18m 23s                                                   │
│ 14:32  o  Read /src/auth.rs (8.1k)                                                                          │ turns      12                                                        │
│ 14:32  *  Session initialized (claude-sonnet-4-6)                                                           │                                                                      │
│                                                                                                             │ ───────────────────────────────────────────────────────────────────  │
│                                                                                                             │ > human  fix line 42, add test case Z                                │
│                                                                                                             │ * agent  Done -- see PR for details                                  │
└─────────────────────────────────────────────────────────────────────────────────────────────────────────────┴──────────────────────────────────────────────────────────────────────┘
 1/1038 · [↩] details · [d] dispatch · [y] approve · [n] reject · [v] transcript · [i] interrupt · [p] prs · [e] errors · [O] issues · [/] search
```

**Urgency mapping for sessions:**
- `review` / `blocked` → High (orange) — needs human action, same tier as "PR needs review"
- `in-progress` → neutral/Low — visible for monitoring, not demanding attention
- `backlog` / `ready` — filtered out in the TUI display layer (not in `workflows/`), same as how
  draft PRs are hidden from certain views; `StatusReport` carries them, the list just doesn't
  render them unless the user explicitly filters to show all agent tasks

**Keybindings:**
- `d` — dispatch modal → ENTER → spawn → auto-selects new session item in list
- `y` / `n` — approve / reject (when session is in `review`)
- `v` — open full transcript in tmux split
- `i` — interrupt agent
- `Esc` — collapse detail panes, return to list-only

**Human-agent collaboration:**
- Both sides write to `task_comments`; agent reads comments on resume via CLI
- `task_activity` is the shared immutable audit trail
- Agent creates a markdown log per task, linked back via `doc_links`

**Task type routing:**
- `implement | debug | general` → determines which skill loads at dispatch
- Extends hub's existing investigation routing model

**SCHEMA_VERSION (do not skip):**
Adding `AgentSession` to `StatusReport` is a breaking schema change. Before committing,
bump `SCHEMA_VERSION` in `workflows/src/status.rs` (currently `14`). The rules for when a
bump is required are in `ui/tui/README.md`. Skipping this causes the TUI to silently serve
a stale cache that doesn't contain session data.

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

## Execution slices

Sliced vertically — each slice produces something testable in the TUI.
Validation is always via the TUI, not the terminal. Thin as possible, but e2e with real permanent logic.

### Slice 1 — Agent sessions visible in TUI ← do this next
1. `tasks` and `task_comments` tables in `store/` using `ensure_table` pattern
2. `hub tasks create` and `hub tasks ready` — minimum CLI to bootstrap test data
3. `AgentSession` domain type in `domain/src/lib.rs`
4. `StatusItem::AgentSession` variant in `workflows/src/status.rs`; bump `SCHEMA_VERSION`
5. Workflow reads in-progress tasks from store + session stats from `.claude/session-stats/`
6. TUI: `Agent` list items, urgency-ranked; `backlog`/`ready` filtered in display layer
7. TUI: split detail view — stream pane (bottom left, JSONL, live-polls 10s) + task info pane (bottom right)

→ Validate in TUI: `hub tasks create --title "test"`, `hub tasks ready TASK-0001`, set `agent_session_id` via SQL, open TUI

### Slice 2 — Full dispatch and agent collaboration loop
8. `hub tasks dispatch` — atomic claim, deterministic session ID, capacity check, spawn Claude Code
9. `hub tasks get / update / comment` — agent-facing commands for reading task and signalling progress
10. `y` / `n` TUI keybindings to approve / reject tasks in `review`

→ Validate in TUI: press `d` on a ready item, watch session appear, agent completes, press `y` to approve

### Slice 3 — CI failure + task link
11. Show `TASK-XXXX` badge on CI failure rows with a linked task
12. "Create task" action on CI failure rows with no linked task

→ Validate in TUI: see badge on a CI failure row that has a linked task

### Slice 4 — Home screen tiles (after Slices 1-2 proven out)
13. KPI summary cards per signal (PR, CI, Linear, Loki, Agent tasks)
14. Two-tier layout: cards top + urgency list bottom-right + agent tasks card bottom-left
15. Per-card urgency thresholds (see status card reference in inspiration doc)

→ Validate in TUI: see home screen with all signal cards and urgency list

---

## On issue #45

Issue #45 ("TUI as control plane") covers the jobs panel, transcript access, and abort — a subset of Phase 3. This roadmap expands scope significantly (task store, CLI, dispatch loop). Consider updating #45 with the full scope or closing it as superseded when starting implementation.
