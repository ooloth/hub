# Inspiration: a personal control-center project

Raw itemization of ideas from a personal control-center project with a web dashboard + agent
CLI + SQLite backend. Goal: apply the best of these to hub's TUI. No filter — big and small.

Agent task management patterns are in a separate doc (`2026-06-02-agent-task-management.md`).
This doc covers everything else.

---

## Part 1: Architecture

### Data Model and Schema

- Comma-separated fields (`jira_tickets`, `pr_links`, `doc_links`) for 1:N resource links — deduplicated on write, split on read, no join table needed for small collections
- Soft deletes via status field (`archived`, `deleted`) — never hard-delete records
- Numbered SQL migration files applied automatically on connection, tracked in a `schema_version` table
- WAL mode + foreign keys enabled on every SQLite connection
- Immutable activity log table: every state change recorded with actor, action, JSON detail, timestamp — full audit trail without touching the main record
- SQLite `GENERATED ALWAYS AS (...) STORED` for derived keys — `TASK-0042` computed at insert from integer ID, indexed, immutable, no application logic needed
- Dataclass + `from_row()` / `to_dict()` pattern: `TABLE_COLUMNS` tuple, typed row mapping, JSON serialization — type safety without an ORM
- Schema validation at startup returns warnings (not exceptions) — loud but doesn't crash; operator gets a chance to diagnose before serving traffic
- Flexible task reference resolution: accept both integer IDs and `TASK-XXXX` format — user-friendly references, simple DB
- Read-only vs read-write DB handles documented explicitly — dashboard is a consumer, tools layer owns migrations
- Snooze state and other ephemeral mutable state in JSON sidecar files (`data/pr-snooze.json`) — not in DB (too ephemeral), but survives restarts

### Caching

- File-based cache with TTL: `{"timestamp": <unix>, "data": <payload>}` — lazy expiration on read, no cleanup needed
- Each data source tracks `is_refreshing`, `last_refreshed`, `last_error` — surfaces staleness and errors independently per source
- `last-refreshed.json` persists refresh timestamps across restarts — app skips re-fetching recently-refreshed sources on startup
- Refresh state also persisted to disk (`data-sources.json`) — re-launching doesn't lose interval config or last-refresh times
- Cache invalidation via file deletion — idempotent, decoupled, debuggable by looking at the filesystem
- Multi-TTL caching: short TTL (300s) for current-state data, long TTL (7200s) for slower/broader scans — different signals decay at different rates
- Two-layer cache for calendar: frontend 5min in-memory + backend 45min file-based — two misses required before a real network fetch
- Large file size guard on cache reads (`max_bytes=500_000`) — never accidentally load a multi-MB cache file into memory
- No external cache service — everything in local files

### Live Updates and Scheduling

- `DataSourceManager`: registers async fetch functions with per-source configurable intervals, `default_interval` (immutable) and `interval` (mutable, persisted to JSON)
- Per-source async lock for independent refreshes — sources refresh in parallel without stomping each other
- Per-group serialization lock when sources share state — prevents thundering herd on related sources without blocking unrelated ones
- SSE event stream for refresh events: each source emits `{type, key, timestamp, error}` — consumers know exactly which source changed
- SSE queue uses non-blocking `put_nowait` — a slow consumer never blocks the refresh loop
- Lazy SSE connection: wait 3 seconds after startup before opening the event stream — lets initial data fetches complete first
- Startup strategy: refresh only sources that are stale (older than their interval), serve cached data for the rest — don't refetch everything on launch
- Per-source refresh intervals tuned to volatility: critical sources (PRs, CI) every 5 min; slower sources (tickets, tasks) every 30 min
- Polling beats WebSockets for a single-machine dashboard: simpler, no connection management, survives restarts
- Subprocess calls always use timeout + returncode check — no hanging calls, no silent failures
- Errors in one data source don't block others — each source handles its own errors and marks itself errored independently
- Retry not baked in at the scheduler level — sources that fail show `last_error`; user triggers manual refresh
- Adding a new data source: write fetch function, register with `manager.register(key, label, fn, default_interval)` — config auto-persisted, cache auto-managed, SSE auto-emitted

### Configuration

- All env vars namespaced with a prefix (`DASHBOARD_*`, `HUB_*`) — no conflicts, grepping is unambiguous
- Each module declares its own env vars at module load time with fallback to repo-relative paths:
  ```
  CACHE_DIR = Path(os.environ.get("DASHBOARD_CACHE_DIR", ""))
  if not CACHE_DIR:
      CACHE_DIR = Path(__file__).resolve().parents[4] / "data" / "cache"
  ```
- Layered `.env` loading: repo-level secrets first, then workspace-level — per-machine overrides without merge conflicts
- Two classes of config: static (rarely changes, lives in code as dicts) vs mutable (user-tuneable, lives in JSON file):
  ```
  Static (code):       API_TO_REPO = {"internal-api": "service-a", ...}
  Mutable (JSON file): data/config/data-sources.json → {"intervals": {"prs": 600}}
  ```
- Config flow:
  ```
  hub.toml / code constants
      ↓ env vars (HUB_*)
      ↓ fallback to repo-relative paths
      ↓ database (mutable repo metadata)
      ↓ JSON config files (intervals, last-refreshed timestamps)
      ↓ file cache (per-key .json files)
  ```
- Group locking for related data sources: sources in the same group share a lock so they don't fetch concurrently
- Token files in `~/.service/token` with clear error messages if missing: exact path + link to generate — no silent auth failures
- Environment variable for config dir with fallback to relative path from module root — portable without being rigid

### CLI Design

- CLI is the single source of truth for business logic — dashboard backend calls CLI via subprocess, never reimplements the logic
- Structured single-line output for machine parsing: `TASK_CREATED TASK-0042`, `DISPATCHED TASK-0042 <id>`, `NO_READY_TASKS`, `AT_CAPACITY`, `RESUMED TASK-0042 <id>`
- Errors to stderr, exit code 1, actionable message: `ERROR: task TASK-0042 status is 'in-progress', expected 'ready'`
- Dry-run mode: `DRY_RUN VERB RESOURCE [key=value ...]` — same output shape, no side effects, exit 0
- `AT_CAPACITY` and `NO_READY_TASKS` exit 0 — expected states, not errors; callers branch on the verb, not the exit code
- `--format=json` flag for scripting — same data, structured as JSON object or array
- Commands accept both human-friendly (`TASK-0042`) and machine-friendly (integer ID) references
- Commands designed for subprocess consumption by the dispatch loop and dashboard backend, not just human use
- Dashboard calls CLI via `asyncio.create_subprocess_exec()`, parses stdout, raises on non-zero exit — never swallows errors
- API field pruning via `?fields=a,b,c` query param — fetch only what's needed, reduce payload
- Parallel repo/API discovery with bounded thread pool (20 workers) — fast enough without overwhelming rate limits
- Agent instructions explicitly state "use the CLI, don't call APIs directly, don't attempt workarounds if a command fails" — prevents agents from diverging from canonical tool behavior
- Hub's CLI is currently a skeleton (per decision #010 it becomes agent-facing) — task management commands are the right first surface to build out: `hub tasks create/ready/dispatch/get/list/update/comment`

### Testing

- Isolated temp SQLite DB per test — no mocking of internal logic, real migrations run
- Test both success and precondition-failure paths (e.g. claiming a task that isn't `ready`)
- Test deduplication: adding a duplicate ticket/PR/doc → same state, no duplicate
- CLI tested via `CliRunner` equivalent — invoke handler directly with test DB path
- Autouse fixtures for cross-cutting concerns (`@pytest.fixture(autouse=True)`) — inject temp cache dir or DB path into every test in a module without explicit parameter passing
- Inline schema in test fixtures via raw SQL (`executescript`) rather than running migration files — faster setup, no file I/O per test
- Patch module-level functions at test time (`monkeypatch.setattr("routes.tasks.get_connection", ...)`) rather than parameterizing constructors — simpler test code
- `TestClient(app)` end-to-end through HTTP routing, not mocking the handler directly — ensures middleware, serialization, and error handling are exercised
- Always assert HTTP status code before drilling into response body
- Mock subprocess at the `asyncio.create_subprocess_exec` layer, not the CLI boundary — mock returns what the tool would return (raw text), the route parses it
- `AsyncMock` with `communicate.return_value = (stdout.encode(), b"")` for subprocess mocks; smart mock routing via `async def fake_exec(*args)` that switches on command args for multi-tool workflows
- `asyncio.get_event_loop().run_until_complete()` to drive async code synchronously in tests
- Call counter pattern (nonlocal variable) to verify caching without mocking time — first call hits the source, second serves cache, assert `call_count == 1`
- Pre-populate cache with `file_cache.set(key, data)` before the request; verify route uses it
- Class-based test grouping (`class TestFoo:`) for related cases; inline assertions for small input sets rather than `@pytest.mark.parametrize`
- Override `datetime.now()` for date-dependent tests by monkeypatching a subclass at module level — not a global mock
- Coverage philosophy: test all public routes (success + common error cases + deduplication); don't test third-party library internals or exhaustively cover every edge case in helpers

### Task Schema Reference (full migration history)

The tasks table evolved through 14 migrations. The key inflection points:

- **Migration 004** — initial schema: `id TEXT PRIMARY KEY` (ULID), plus `title`, `description`, `status`, `priority`, `assignee`, `created_by`, `parent_id`, `jira_tickets`, `pr_links`, `tags`, `created_at`, `updated_at` + `task_comments` + `task_activity`
- **Migration 005** — switched from TEXT (ULID) to `INTEGER PRIMARY KEY AUTOINCREMENT` — required full table recreate (SQLite can't alter PK type)
- **Migration 006** — added `task_key TEXT GENERATED ALWAYS AS ('TASK-' || SUBSTR('0000' || id, -4)) STORED` — zero-padded 4-digit key, computed at insert, indexed with `CREATE UNIQUE INDEX`
- **Migration 007** — added `doc_links TEXT`
- **Migration 010** — added `agent_session_id TEXT`
- **Migration 011** — removed `priority`, `assignee`, `created_by`, `tags` — these were over-engineered; the final schema is leaner
- **Migration 012** — added `type TEXT` (nullable, e.g. `'implement'`, `'debug'`, `'general'`)

**Final tasks table** (after migration 011 + 012):
```sql
CREATE TABLE tasks (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    title            TEXT    NOT NULL,
    description      TEXT,
    status           TEXT    NOT NULL DEFAULT 'backlog',
    parent_id        INTEGER REFERENCES tasks(id) ON DELETE SET NULL,
    jira_tickets     TEXT,   -- comma-separated: "DWO-123,DWO-456"
    pr_links         TEXT,   -- comma-separated URLs
    doc_links        TEXT,   -- comma-separated relative paths
    created_at       TEXT    NOT NULL,
    updated_at       TEXT    NOT NULL,
    task_key         TEXT    GENERATED ALWAYS AS ('TASK-' || SUBSTR('0000' || id, -4)) STORED,
    agent_session_id TEXT,
    type             TEXT
);

CREATE UNIQUE INDEX idx_tasks_task_key ON tasks(task_key);
CREATE INDEX idx_tasks_status        ON tasks(status);
CREATE INDEX idx_tasks_parent_id     ON tasks(parent_id);
```

**task_comments table:**
```sql
CREATE TABLE task_comments (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author     TEXT    NOT NULL,   -- "human" or "agent"
    content    TEXT    NOT NULL,
    created_at TEXT    NOT NULL
);

CREATE INDEX idx_task_comments_task_id ON task_comments(task_id);
```

**task_activity table** (immutable audit log):
```sql
CREATE TABLE task_activity (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    actor      TEXT    NOT NULL,   -- "human", "agent", "system"
    action     TEXT    NOT NULL,   -- "created", "status_change", "updated", "comment"
    detail     TEXT,               -- JSON: {"from": "ready", "to": "in-progress"}
    created_at TEXT    NOT NULL
);

CREATE INDEX idx_task_activity_task_id ON task_activity(task_id);
```

**SQLite PRAGMAs on every connection:**
```python
conn.execute("PRAGMA journal_mode=WAL")
conn.execute("PRAGMA foreign_keys=ON")
```

**Migration runner** auto-applies on connection, splits each `.sql` file on `;`, strips comment lines, commits each migration individually, tracks applied versions in `schema_version` table.

**`Task` dataclass** mirrors the table with `TABLE_COLUMNS` tuple + `from_row()` + `to_dict()`. `to_dict()` omits `None` fields (sparse output). The `detail` field in `TaskActivity.to_dict()` attempts `json.loads()` before falling back to raw string — so JSON detail is always returned parsed, never double-encoded.

**Design decisions worth noting:** Priority, assignee, and tags were added and then removed (migrations 004 → 011). The final design is intentionally minimal — status + type + linked resources. The `parent_id` self-reference supports subtasks but was never heavily used in practice.

---

## Part 2: UI/UX Patterns

### Screen and Navigation Architecture Reference

The web dashboard is a single React app with a persistent 220px left sidebar and a main content area. Every page lives under a shared `Layout` component (Mantine `AppShell`). There are 18 nav items in 5 groups separated by dividers:

```
Overview
─────────────────────
To Do | Notes | Projects | Docs
─────────────────────
Agents | Agent Config
─────────────────────
Chemist Config | Flux Errors | Flux E2E Tests | Pull Requests | DTS Tickets | DWO Tickets
─────────────────────
Links | Documents | Books | People
─────────────────────
Settings
```

**Three page architectures exist:**

1. **Full-page** (most pages) — the route fills the main area. No persistent sub-navigation. Examples: PRs (table + right drawer), Tickets (table), E2E Tests (table + chart).

2. **Nested routes with left sub-nav** — page has its own sidebar + right content area; URLs are `/parent/child`. Examples: Settings (`/settings/general`, `/settings/data-sources`, `/settings/repositories`), Chemist Config (`/centaur-config/flux-models`, `/centaur-config/argocd`, etc.), Links (dynamic sub-pages from API).

3. **Three-column** — left list + center detail + right content. Only the Agents page uses this. Left: agent card list (280px). Center: task info pane (360–500px). Right: chat/JSONL display (flex 2).

**Overview page** is architecturally distinct from all others: it's a grid of independent `StatusBox` components, each fetching its own endpoint. A shared `refreshKey` counter triggers all boxes to re-fetch simultaneously. Each box calls `onRefreshStart()` / `onRefreshEnd()` callbacks so a global spinner knows when the last one finishes.

**Navigation state:**
- Active sidebar item: `location.pathname.startsWith(item.path)`
- Sub-page state (selected tab, active item): `useSearchParams()` — survives refresh, shareable URLs
- Cross-navigation state: **not preserved** (lost on page change) except draft notes in `localStorage`
- Auto-refresh intervals vary widely per page: Agents (10s polling), E2E (5min), nav badges (5min), most pages (manual only)

**Badge system:** A `useNavBadges()` hook fetches all badge counts every 5 minutes and listens for a `refresh-badges` window event. Any page can fire that event after a mutation to keep the sidebar in sync without knowing about nav internals.

**For hub's TUI:** The key insight is that hub already has a TUI analogue of this: a unified list (home screen) + detail screens. The three-column Agents layout maps naturally to hub's planned split panel (urgency list + running agents). The nested-routes pattern maps to hub's modal/popup layers. The `refreshKey` broadcast pattern maps to hub's existing `refresh_key` or equivalent event loop signal.

### Layout and Navigation

- Three-pane layout: compact list (fixed width) + scrollable detail pane + scrollable chat/activity pane — each scrolls independently
- Detail panel opens alongside the list (split view), not replacing it — list stays visible for comparison and quick jumping:
  ```
  ┌──────────────────┬────────────────────────────────────┐
  │ List             │ Detail                             │
  │ ● item A   2d   │ Item A                             │
  │ ● item B   5d   │ Author · Status · Age              │
  │ ● item C   1d   │ ─────────────────────────────────  │
  │                  │ Activity                           │
  │                  │ ✓ approved  3h ago                 │
  │                  │ 💬 comment  1d ago                 │
  └──────────────────┴────────────────────────────────────┘
  ```
- Ranked signal list alongside a jobs/tasks panel — two concerns visible simultaneously without switching screens
- Segmented control / tab switching within a screen encodes selected tab into navigable state — preserves context on back-navigation
- Breadcrumbs above detail titles so the user knows where they are and how to go back
- Nav sidebar badges (count of urgent items per section) update independently on their own polling interval
- Force-refresh of badges via event dispatch — any screen can trigger a global badge refresh without knowing about the nav component
- Type-specific UI: different data shown for different item types (e.g. CI failure items show log excerpt inline; PR items show review status)

### Lists

- Sort by most recent update by default — surfaces items that need attention, not just newest-created
- Section headers always show item count in parentheses: "Needs review (5)" — scope visible at a glance
- Color-coded age badges inline in list rows: green ≤2d, yellow 3–6d, red ≥7d — staleness visible without reading the date
- Category grouping into sections, not kanban columns: "Mine / Needs review / Approved / Team" as tabs — simpler than drag-and-drop swim lanes
- Sections ordered by urgency: "mine" first, then "unassigned/needs action", then "completed/FYI" — trains the eye to scan top-down
- Deduplication with precedence across sections: if an item appears in "mine", remove it from "recent" — never show the same item twice
- Unassigned items highlighted in bold red — "needs an owner" is a first-class signal
- Fixed-width columns with explicit sizing — consistent layout even with variable-length content
- Row striping + highlight on focus — reduces eye travel across wide rows
- Snooze mechanism: hide items from urgency counts and badge totals until new activity occurs; auto-unsnooze when the item updates
- "New since snooze" marker: highlight activity that arrived after the item was snoozed — surfaces only the delta
- Snoozed rows at 45% opacity — present but visually suppressed
- Draft/state badges inline next to the item ID, not in the title column:
  ```
  #123 [Draft]     #456 [Snoozed]
  ```
- Comment count as activity proxy: 0 = fresh/unblocked, 5+ = active discussion or complex

### Detail Views

- Linked resources grouped by type (tickets, PRs, docs) with icons and live-fetched state badges (PR open/merged/declined)
- PR status badge fetched dynamically per linked URL — doesn't rely on stored state going stale
- Activity timeline with type-specific icons (comment=blue message, approval=green check, changes-requested=orange warning, push=gray git-commit)
- Post-snooze activity highlighted with left border + contrasting background — separates pre-snooze from post-snooze at a glance:
  ```
  │ ← colored border
  │ [New] ✓ approved  1h ago
  │   "All good, merging tomorrow"
  ```
  Footnote: "Activity above marked 'New' occurred after snooze"
- Activity truncated: comments to 300 chars, commit messages to first line / 200 chars — never fills the pane with a wall of text
- Update events synthesized into human-readable summaries: "Pushed commit abc1234", "Marked as draft", "Title updated: new title" — not raw API event objects
- Two-line header: repo/context name (small, dimmed) above item title (bold) — context without eating column width
- Context % rendered as a color-coded horizontal bar: green <75%, yellow 75–90%, red >90% — agents approaching limit is immediately obvious
- Formatted elapsed time: "2m 30s", "45s", "2d ago", "3h ago" — not raw timestamps
- Monospace font for technical values: session IDs, file paths, command strings, identifiers
- Inline URL detection in description text: render as clickable links, not plain text
- Truncation indicator when content is cut off — never silently hide data

### Rendering Agent Activity

- Three distinct block types for agent tool calls: diff blocks (old/new with −/+ prefixes), bash command blocks, file write blocks
- Diff blocks show `old_string` vs `new_string` as two sections with colored prefixes (red for removed, green for added)
- Cap JSONL parsing at N messages — safe, incremental, never loads a huge session into memory all at once
- Filter out system messages from JSONL before rendering — only show user-facing tool use and responses
- JSONL parsing is line-by-line, not full-file parse — handles large or still-growing session files

### Status and Health Indicators

- Semantic color map — consistent everywhere, never ad hoc:
  ```
  backlog=indigo  ready=blue  in-progress=cyan  blocked=red
  review=orange   done=green  archived=dimmed
  ```
- Global urgency color convention: red = action required, yellow/orange = attention, green = clear, gray = neutral/backlog
- Status text always reinforces the color in words — redundant encoding for colorblind users and terminal themes
- KPI summary cards: large primary count + small label + icon watermark at low opacity — high-information density, scannable at a glance:
  ```
  ┌──────────────────────┐
  │ PRs to Review        │
  │         3            │  ← large, red if >0
  │  awaiting review     │
  │  1 draft             │  ← secondary detail
  └──────────────────────┘
  ```
- Primary count color codes urgency; secondary detail line gives context without competing with it
- Reserve space with a placeholder when secondary detail is empty — no layout shift when data loads
- "!" on a single card when its source errors — other cards unaffected, error is localized
- Timeline chart for trend visibility: 30-day dual-axis chart (failures count + duration) — detects degradation that point-in-time status misses
- Color-coded dots at each chart data point (green=pass, red=fail) in addition to line color — scan for red is instant
- Reference line at average duration on runtime chart — immediately shows whether a run is an outlier
- Per-item health computed from multiple signals, not a single field — e.g. PR urgency from age + review status + CI status combined

### Empty and Loading States

- Placeholder non-breaking space when no data — maintains vertical alignment without layout shift
- Show "?" as placeholder while data loads for the first time — communicates "coming soon" vs "none"
- Dimmed color for "no items" text — visually distinct from real data, not an error state
- Spinner during initial load; smaller spinner during incremental re-fetch — communicates different states differently
- Loading spinner with 3-second timeout: if no data arrives, show "No sources registered" — never spin forever
- Stale data badge: "Stale — last run YYYY-MM-DD" in yellow when data exceeds age threshold — never silently serve old data
- `LoadingOverlay` on in-place refresh — content stays visible but dimmed; no flicker

### Forms and Interactions

- Dirty flag on edit forms — save button disabled until a change is made AND required fields are non-empty
- Immediate saves — no "Save" button; changes take effect on Enter/confirm
- Optimistic updates: update local state immediately on drag/reorder, confirm with server asynchronously, roll back on next fetch if rejected
- Two-stage confirm for destructive actions: first action shows "Confirm?" prompt, second executes — prevents accidents
- Autocomplete for category/type fields using existing values — reduces typos, surfaces valid options without a closed enum
- Copy-to-clipboard with feedback: icon changes from copy to checkmark, tooltip shows "Copied!" — micro-confirmation
- Drag-and-drop with drag-counter pattern: `dragCounter++/--` to only highlight drop target when actually over it, not just over a child element

### Keyboard and Interaction Patterns

- Escape closes the current modal/popup — implement as highest-priority key in the event loop; nested popups each handle their own Escape (last-opened closes first)
- Enter submits the focused form field without requiring a "Save" button click
- Inline editing: press a key to switch a display field to an edit field with autofocus; Enter saves, Escape cancels — no separate modal for single-field edits
- Two-stage confirm for destructive actions: first keypress shows "Confirm?" prompt, second executes
- Drag-and-drop status change in the web app → in TUI: arrow keys to move between status columns, Enter to confirm move
- Search field clears on Escape — consistent with modal-close behavior, muscle memory transfers
- Type-to-filter in autocomplete fields — maps directly to TUI filter-as-you-type
- Selected item highlighted with border/background color change — unambiguous current position in list
- "?" overlay showing available shortcuts — the web app doesn't have this; TUI should
- Footer hint bar showing context-sensitive keys for the current screen — reduces discoverability gap
- No global application hotkeys in the web app (it's mouse-first) — TUI inverts this: every action should have a key, mouse is optional

---

## Part 3: Feature Screens

### Home / Overview

- Fixed-width status cards (one per signal category) in a flex-wrap grid — add/remove signals by toggling visibility, layout adapts automatically
- Card grid rows grouped by theme (CI row, work items row, schedule row) — visual hierarchy without headers
- One summary endpoint returns all badge counts in parallel — prevents N separate fetches for the sidebar
- Sidebar nav badges computed from the same summary data: red = action required, orange = attention, hidden = clear — trains users to scan red first
- Badge refresh on a separate timer (every 5 min) independent of page content refresh
- Event-driven badge refresh: any screen can fire a refresh event after an action; the badge hook re-fetches — sidebar stays in sync without a full reload
- Fetch one full dataset per source, slice it in the consumer — no separate endpoints for "mine" vs "team" vs "needs review"
- Multi-source deduplication with precedence: if an item appears in "approved pending", remove it from "needs review" — never show the same item in two urgency categories
- "Also check" cross-signal grouping: combine two related signals into one card — communicates "fix these together"
- Pending counter for coordinated multi-source refresh: each source increments on start, decrements on finish; spinner clears only when counter hits zero
- Time-horizon bucketing for personal work: "today" vs "this week" vs "backlog" — urgency encoded in the category, not just a priority field
- Hub-specific home layout sketch — two-tier architecture: cards for glanceable summary (top), urgency list for ranked detail (bottom-right), agent card (bottom-left):
  ```
  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐
  │ PULL REQUESTS │  │ GITHUB CI     │  │ LINEAR ISSUES │  │ LOKI ALERTS   │
  │      7        │  │      0        │  │      3        │  │      2        │
  │ ON TRACK      │  │ ALL PASSING   │  │ ON TRACK      │  │ FIRING (HIGH) │
  │ 2 rev/3 my    │  │               │  │ 2 mine/1 unasn│  │ prod-api (2)  │
  └───────────────┘  └───────────────┘  └───────────────┘  └───────────────┘
  ┌───────────────┐  ┌────────────────────────────────────────────────────────┐
  │ AGENT TASKS   │  │ URGENCY RANKING                                        │
  │      1        │  │ 1. [CRITICAL] PR#42 · alice/feature · 23m ago          │
  │ IN PROGRESS   │  │ 2. [HIGH] CI · cargo check · owner/repo · 45m ago      │
  │ 1 task active │  │ 3. [HIGH] Loki (3) · OOM killed · prod-api · 1m ago    │
  └───────────────┘  │ ↓ scroll  p=PRs  e=Errors  i=Issues  /=search         │
                     └────────────────────────────────────────────────────────┘
  ```
- All-clear state: all cards green, urgency list shows celebratory message + countdown to next refresh
- Card spec per signal type: primary count (large, red if nonzero urgent), status text (reinforces color in words), secondary detail line (breakdown: "2 review / 3 mine / 2 draft")
- Ratatui layout: `Constraint::Length(29)` per card, `Constraint::Min(0)` for urgency list pane

### Status Card Urgency Thresholds Reference

Each card has a specific primary count, urgency threshold, and secondary detail. These are the exact rules from the inspiration project, useful as a reference when designing hub's equivalent:

| Card | Primary count | Red (urgent) | Orange (attention) | Green (clear) | Secondary detail |
|---|---|---|---|---|---|
| **Flux Errors** | Unresolved runs | `unresolved > 0` | — | `== 0` | `N total failures` |
| **Flux Models** | Versions behind | `versions_behind > 0` | `untagged_commits > 0` (only) | both == 0 | Version transition `old → new` |
| **E2E Tests** | Failure count | `result != SUCCESSFUL` (and not snoozed) | — | `SUCCESSFUL` | Test date YYYY-MM-DD |
| **DTS Tickets** | Assigned to me | `mine > 0` | — | `== 0` | Unassigned count (red if > 0) |
| **PRs to Review** | Non-draft, non-snoozed | `count > 0` | — | `== 0` | Draft count |
| **Agent Tasks** | Tasks in `review` | `review > 0` | — | `== 0` | `N ready · N in-progress` |
| **To Do (today)** | `status == "today"` | — | `today > 0` | `== 0` | `N this week · N backlog` |
| **ArgoCD** | Out-of-sync resources (prod only) | — | `oos > 0` | `== 0` | Unhealthy count (red if > 0) |
| **DWO Sprint** | Unassigned in sprint | `unassigned > 0` | — | `== 0` | Mine count (blue) |
| **My PRs** | All authored | informational only | — | — | — |
| **Approved PRs** | Awaiting merge | informational only | — | — | — |
| **Gmail** | Unread | informational only | — | `== 0` | Total in inbox |
| **Bookmarks** | Unread links | informational only | — | `== 0` | Link to service |

**Sidebar badge rules** (derived from the same data, shown as nav badges):
- RED badge: Flux Errors (unresolved > 0), E2E Tests (failing + not snoozed), DTS Tickets (mine > 0), DWO Tickets (unassigned > 0), Agent Tasks (review > 0), PRs (non-draft non-snoozed > 0), Chemist Config (argocd oos > 0 OR versions_behind > 0)
- ORANGE badge: To Do (today > 0), Chemist Config (untagged_commits > 0 only, no other issues)
- No badge: My PRs, Approved PRs, Gmail, Bookmarks, Books, Calendar

**For hub's equivalent thresholds:**
- **PRs to review** → red if any non-draft, non-snoozed PRs need review; secondary: draft count
- **GitHub CI** → red if any monitored workflow is failing; secondary: failed repo names
- **Linear** → red if any assigned issue is overdue or urgent; orange if unassigned in cycle; secondary: mine vs unassigned counts
- **Loki alerts** → red if any critical/high alerts firing; orange if medium; secondary: alert names with counts
- **Agent tasks** → orange if any in `review` (human action needed); secondary: in-progress + ready counts
- The "informational only / blue" distinction is worth preserving — not every count needs to be an alarm

### PRs

- Four sections in the PR list, each independently scrollable:
  ```
  Needs my review (3)    My PRs (2)    Approved, awaiting merge (1)    Team requests (4)
  ```
- Per-row columns: Repo, PR#, Title, Comments, Updated, Author, Created, Snooze toggle — fixed widths, ellipsis on overflow
- Age badge coloring on both Updated and Created columns: green ≤2d, yellow ≤6d, red ≥7d
- Snooze stores `updated_on`, `comments`, and `source_hash` at snooze time; auto-unsnoozes if any of those change on next fetch — tracks the delta, not just time
- Right-side detail drawer opens on selection, list stays visible on left (see Layout section for diagram)
- Activity timeline in drawer: icon + colored badge per event type
- Post-snooze activity highlighted (see Detail Views section)
- Parallel source aggregation: multiple git hosting providers merged by URL, "approved pending" takes precedence over "needs review" if same PR appears in both — no duplicates, no ghost PRs
- Discover mode for PR scanning: scan all repos updated in last 14d, union with whitelist, subtract blacklist — cached separately at longer TTL than core repos

### Snooze Persistence Reference

Snooze state lives in a JSON sidecar file (`data/pr-snoozes.json`, path configurable via env var). It is **not** in the database — too ephemeral, but does need to survive restarts.

**Sidecar file structure** (keyed by PR URL):
```json
{
  "https://github.com/owner/repo/pull/123": {
    "snoozed_at": "2026-05-19T10:00:00+00:00",
    "updated_on": "2026-05-18",
    "comments": 5,
    "source_hash": "abc1234def567..."
  }
}
```

- `snoozed_at` — when the user clicked snooze (ISO 8601 with timezone)
- `updated_on` — snapshot of the PR's `updated_on` field at snooze time
- `comments` — snapshot of PR's comment count at snooze time
- `source_hash` — snapshot of source commit hash at snooze time

**Auto-unsnooze logic** — runs on every PR list fetch, checks three fields:
```python
if (
    pr.get("updated_on") != entry.get("updated_on")  # PR was updated
    or pr.get("comments") != entry.get("comments")   # New comments
    or pr.get("source_hash") != entry.get("source_hash")  # New commits
):
    del snoozes[url]  # Auto-unsnooze
```

Auto-unsnooze is **event-driven, not TTL-based** — a PR can stay snoozed indefinitely if nothing changes. The `dirty` flag triggers a file write only when at least one snooze was cleared, avoiding writes on every fetch.

**API endpoints:**
- `POST /api/prs/snooze {"url": "..."}` — snapshots current PR state, writes to sidecar, returns updated PR list
- `DELETE /api/prs/snooze {"url": "..."}` — removes from sidecar, returns updated PR list
- Both endpoints respond with the full updated PR data, so the frontend doesn't need a follow-up GET

**Badge count filtering:** `needs_review.filter(pr => !pr.draft && !pr.snoozed).length` — both drafts and snoozed items excluded from the urgent count. Snoozed items remain in the list at 45% opacity with a purple badge.

**"Since snooze" delta:** Frontend passes `snoozed_at` as a query param to the activity endpoint. Backend tags each activity item with `since_snooze: bool` by comparing `item.created_on > snoozed_at`. Items after the snooze time get a violet left border + "New" badge + footnote.

**Edge cases:**
- PR merged while snoozed → snooze entry becomes orphaned in the file; harmless since the URL won't match any live PR
- Legacy format (bare timestamp string) → auto-migrated to new structure on read; bare `updated_on` triggers immediate auto-unsnooze
- `source_hash` comparison is stored but currently behaves as always-different (field rarely populated) — effectively dead code or a future feature

**For hub:** Hub's signals (PRs, CI runs, Linear issues, Loki alerts) would each benefit from snooze. The sidecar file approach works well — one file per signal type, keyed by whatever uniquely identifies the item (PR URL, run ID, alert fingerprint). The auto-unsnooze trigger fields would differ per signal type (e.g. for a CI run: re-triggered? for a Loki alert: resolved? for a Linear issue: status changed?).

### Tickets / Issues

- Hierarchical categorization by context: "mine" (assigned to me), "unassigned sprint" (team responsibility), "recent" (any assignee, last 14d) — each is a separate query, merged and deduplicated:
  ```
  Assigned to me (5)
  ├─ WORK-123  Fix auth bug      [In Progress]  1d
  ├─ WORK-124  Update docs       [To Do]        3d
  └─ (3 more)

  Other recent updates (12)      ← deduped: already-mine tickets removed
  ├─ TEAM-456  Setup CI          [To Do]        2d
  └─ (11 more)
  ```
- Lean column set: Key, Summary, Status, Updated, Comments, Type, Priority, Assignee — nothing more
- Status badge color: green=Done, blue=In Progress, gray=To Do — red reserved for alerts only
- Summary column wraps to 2 lines max — enough context without horizontal scroll
- Two refresh modes: cached overview (fast, for status cards) vs force-refresh (slow, for full page)
- Parallel queries with bounded thread pool — all board sections fetch concurrently, each independently handles errors
- Rich text → plain text extraction for ticket descriptions: strip tags, convert `<br>` to newlines, preserve links as `[text](url)`

### Monitoring and CI/Pipeline UI

- "ALL PASS" vs large red failure count as the primary status display — no ambiguity, no scanning required
- Dual-state tabs with counts: "Current (7)" and "Resolved (23)" — filter unresolved vs resolved without separate pages, count always visible
- Sortable paginated run table (PAGE_SIZE=15) with date, run #, result, env, test counts, duration — limits render cost while showing depth
- Run detail panel in a sticky sidebar (1/3 width): selecting a run from the list (2/3 width) opens detail alongside without leaving the list
- Test history drill-down: pipeline → individual test → 30-day history of that specific test across runs
- Dual-axis line chart: failures count on left Y-axis, duration on right Y-axis — spots performance regressions alongside failure trends in one view
- Pass/fail ratio as a pie chart paired with a runtime-over-time line chart, side by side — two complementary views of the same history
- Reference line at average duration on runtime chart — immediately shows whether a run is an outlier
- Variable lookback window: if latest data is stale, extend the window; clamp to `[1, 30]` days — adapts to actual data freshness
- Structured error metadata shown before the raw error dump: step type, workflow name, timestamps — triage without reading the full error
- Monospace code block for error text with max height and scroll — preserves whitespace, searchable, never wraps awkwardly
- Error line-clamping in table cells (3 lines max) — shows enough to recognize the error without blowing out the row height
- "Task already created" indicator: if an error run has a linked task key, show it as a badge; otherwise show a "create task" action — closes the loop between monitoring and work tracking
- Annotation columns added to monitoring records post-hoc (`resolved`, `resolution` enum, `notes`, `task_key`) — triage state layered onto raw run data without schema redesign
- Resolution category as a fixed enum ("User Error", "Transient", "Known Error", "Ignore", etc.) — fast triage, consistent vocabulary
- Freeform notes field with save-on-blur, not save-on-keystroke — avoids thundering herd of DB writes
- Resolved checkbox moves item to "Resolved" tab without deleting — soft-resolve, always undoable
- Daily failure count aggregation: `GROUP BY DATE(started)` → `{date, count}` array — feeds trend charts and summary counts from one query
- Deep-link support: navigable state encodes selected run ID — linkable without a router
- Stepper component for multi-step pipeline state — shows which step is the current blocker visually
- Build/run number as a human-readable link to the external pipeline — `#123` not a UUID

### Todo Board

- 7-column kanban organized by time horizon, not just status — columns are temporal buckets:
  ```
  ┌───────────┬───────────┬──────────┬───────────┬───────────┬─────────────┐
  │ THIS WEEK │   TODAY   │   DONE   │  ARCHIVE  │  MONITOR  │ WORKSTREAMS │
  │   (12)    │    (4)    │   (2)    │  ╌╌╌╌╌╌  │   (3)     │    (5)      │
  │ [card]    │ [card]    │ [card]   │  drop →   │ [card]    │ [card]      │
  │ [card]    │ [card]    │          │  archive  │           │             │
  └───────────┴───────────┴──────────┴───────────┴───────────┴─────────────┘
  ```
  Archive column has dashed border — visual affordance for "drop here to dismiss"
- 4-level priority system communicated via icon + card border color:
  ```
  low    → gray  ↓  (no border)
  normal → blue  =  (no border, default)
  high   → orange ↑ (colored left border)
  urgent → red   !  (colored left border, stronger)
  ```
- Priority picker in edit modal: 4 icon buttons, selected at full opacity, others dimmed — not a dropdown
- Card shows only: title (2 lines, bold), description (2 lines, dimmed, only if non-empty), type badge — everything else in the detail modal
- Type badge uses hashed color from the type string (8 possible colors) — consistent per type without a fixed palette
- Type system is dynamic: types emerge from created items, auto-registered in a `todo_types` table, surfaced via autocomplete — no predefined enum
- Create modal defaults status to `today` when opened from board tab, `backlog` when opened from backlog tab — context-aware default
- Backlog tab shows backlog + next-week side-by-side with a dedicated "promote to this week" drop zone:
  ```
  ┌───────────────────┬───────────────┬───────────┐
  │ BACKLOG (23)      │ NEXT WEEK (5) │ ╌╌╌╌╌╌╌  │
  │ [items]           │ [items]       │ → To This  │
  │                   │               │   Week     │
  └───────────────────┴───────────────┴───────────┘
  ```
- Archive tab is read-only with a "Clear Archive" bulk delete — requires confirmation, irreversible
- Todos and tasks are separate entities in separate databases with separate status vocabularies:
  - Todos: `backlog / next-week / this-week / today / done / monitor / workstreams / archived` — lightweight, personal
  - Tasks: `backlog / ready / in-progress / blocked / review / done / archived` — tracked, linked to tickets/PRs/docs
- Optimistic update on drag: move card in local state immediately, then sync with API, then full refresh

### Calendar / Schedule Integration

- Single-day hour-strip widget (8am–7pm fixed window) as a dashboard card, not a full-page view:
  ```
  ┌─ Monday, 2 June ────────────────────┐
  │ All day: Company Holiday             │
  ├──────────────────────────────────────┤
  │ 08:00 ┌────────────────────────┐    │
  │       │ Daily Standup          │    │
  │       │ 08:00 – 09:00          │    │
  │       └────────────────────────┘    │
  │ 09:00                               │
  │ 11:00 ── now ────────────────────── │  ← red dot + line
  │ 11:30 ┌────────────────────────┐    │
  │       │ Design Review          │    │
  │       │ 11:30 – 13:00          │    │
  │       └────────────────────────┘    │
  │ 19:00                               │
  └──────────────────────────────────────┘
  ```
- All-day events in a separate section above the hour strip — don't clutter the timed grid
- "Now" indicator (red dot + horizontal line) auto-scrolls into view when opening today
- Past events fade to 50% opacity and gray — the past doesn't compete with what's coming
- Minimum event height enforced — prevents unreadably thin blocks for short meetings
- Events clipped to the 8–19 window — anything outside is silently excluded
- Event detail on selection: title, time, organizer, location, video call URL (extracted via regex from description/location fields), cleaned description
- All-day and timed events separated at the data level (`all_day: bool`) — different rendering, not just different fields
- Event exclusion via configurable pattern list — hide low-signal events (lunch, personal time) without deleting them
- Cancelled events filtered out server-side
- Per-date fetching (not range-based) — one cache entry per date, no range bloat
- Timezone-aware parsing server-side, display as HH:MM strings — no client-side timezone math
- Recurring event expansion (RRULE) done server-side — consumer sees a flat list of single-day occurrences
- **Gap worth filling**: calendar and tasks are completely separate silos in this project — no cross-reference between meeting load and task commitments. A TUI could show task deadlines relative to calendar, warn about over-scheduling, or surface free blocks for deep work
- Hub-specific calendar integration ideas (opinionated priority order):
  - Phase 1 — "Next meeting" footer: always-visible single line; dims when no meetings within 8h; yellow < 15min, red < 5min; updates locally every 30s:
    ```
    Next: Design Sync in 1h 23m | 6h 45m free today | c) free blocks
    ```
  - Phase 2 — Inline conflict badge on items with deadlines: "⏰ Client call 3pm" next to the item if a blocking meeting falls before its deadline
  - Phase 3 — Free blocks modal on `c`: gap detection between events, filter slots < 30min, show total:
    ```
    ┌─ Free blocks today ──────────────────┐
    │ Now — 2:15pm      (2h 15m)           │
    │ 3:45pm — 5:00pm   (1h 15m)          │
    │ Total available: 3h 30m              │
    └──────────────────────────────────────┘
    ```
  - Phase 4 — Time pressure visual hint: if < 2h free, dim indicator on High items (not urgency change — urgency is the workflow's job)
  - iCalendar client in Rust: `icalendar` crate, per-date fetch, cache in SQLite alongside status data, 30min TTL
  - `FreeBlock { start, end, duration }` pre-computed at fetch time, stored in `StatusReport`
  - Config: `[calendar] ical_url = "..."`, `blocking_event_patterns = ["call", "meeting", "stand-up"]`
  - Skip: ML prediction, two-way sync, meeting prep checklists — juice not worth the squeeze

### Repo Management

- Three support tiers derived at runtime from two boolean fields (never stored as a tier column):
  ```
  venv_configured=true  AND tests_configured=true  → Fully Supported  (green)
  venv_configured=true  OR  tests_configured=true  → Tracked          (yellow)
  neither                                           → Discovered       (gray)
  ```
- Repo schema: `name, origin, venv (enum), venv_configured, tests_configured, custom_test_handling, notes` — capability bits, not status
- `venv` field is an enum routing hint: `pyproject` (single venv at root), `monorepo` (per-subproject venvs), `none` (skip) — determines workspace setup strategy
- `notes` field captures operational blockers: "needs libomp", "private pip index unreachable", "macOS-only" — explains why a flag is false despite the repo being known; rows with notes shown in warning color
- Edit modal shows derived "Fully Supported" badge that recomputes live as user toggles the two flags:
  ```
  [✓] Venv Configured
  [✗] Tests Configured
  ──────────────────────
  Status: TRACKED  (not fully supported yet)
  ```
- Two separate repo views with different concerns:
  - Metadata repos — all tracked repos, configurable, filterable, stable
  - Git repos — subset with rich commit history, tags, release management; expensive to fetch, cached aggressively
- Git commit graph with Unicode box drawing: main lane vs branch lanes in different colors, release tags as larger nodes, "untagged commits since last release" highlighted as a signal to cut a release
- Origin resolution: DB lookup first, then falls back to platform conventions — warns if origin was guessed
- Host extracted from `origin` field for filtering and badging: `github.com` → GitHub, `bitbucket.org` → Bitbucket
- Batch filter → action pattern: filter list, select multiple, apply update to all selected

### Settings / Data Source Management

- Settings as a dedicated screen with sidebar nav: General, Data Sources, Repositories — each a full subpage, not a modal
- Data source list as a table, one row per source:
  ```
  ┌─ Source ──────────┬─ Status ──┬─ Last Refreshed ─┬─ Interval ─┬─ Data ─┬─ ↻ ─┐
  │ Pull Requests      │ ✓ OK      │ 3m ago           │ 5m         │ 👁     │  ↻  │
  │ Flux Errors        │ ✗ Error   │ 45m ago          │ 10m        │ —      │  ↻  │
  │ E2E Tests          │ 🔄 ...    │ just now         │ 5m         │ 👁     │  ↻  │
  └───────────────────┴───────────┴──────────────────┴────────────┴────────┴──────┘
  ```
- Status badge states: ✓ OK (green), ✗ Error (red), 🔄 Refreshing (blue), ⏳ Never (gray)
- Error badge shows the actual error message on hover/focus — don't bury it in a modal
- Interval column is click-to-edit inline — input appears in place, Enter to save, Escape to cancel; no modal needed
- Interval = 0 means disabled — unifies "pause" and "configure rate" into one field
- Shows both current interval and default: "10m (default: 5m)" when customized — never hides configuration drift
- Data preview button opens a modal showing the raw cached payload — disabled until data exists and is under size limit; reason shown in tooltip when disabled
- "Last refreshed" column updates via a background timer (every 30s) without re-fetching — relative times stay fresh locally
- Per-source manual refresh button always available, always triggers immediately
- Global refresh button triggers all sources in parallel; spinner clears only when all finish (pending counter pattern)

---

## Part 4: Agent System

### Task Type Routing

- Task types (`implement`, `debug`, `general`) guide which skill the agent loads at dispatch — deterministic, no guessing
- Type-specific UI in the detail view — different data shown inline for different task types
- Unrecognized task type → escalate immediately rather than guessing which workflow to run
- Could map to investigation types in hub (e.g. `ci-failure` auto-loads Codefresh context, `pr-review` auto-loads PR diff)

### Agent Dispatch

- Deterministic session IDs via `uuid5(namespace, task_key)` — same task always maps to same Claude Code session, enables resume without lookup
- Capacity management: count `status='in-progress' AND agent_session_id IS NOT NULL` as a semaphore, respect `--max-sessions`
- Dispatch loop is safe to run on a timer — atomic claim prevents double-dispatch
- `NO_READY_TASKS` and `AT_CAPACITY` are expected non-error states, not failures
- `RESUMED` output code when re-dispatching a task that already has a session — no new session created
- Dispatch supports multiple launch targets (different terminal apps) with graceful fallback
- Hub TUI dispatch surface — split panel: no agents running → full-width urgency list; agents running → 50/50 split:
  ```
  ┌─ Urgency List ──────────────┬─ Running Agents ──────────┐
  │ CRITICAL                    │ ⊛ Issue#123  ooloth/hub   │
  │  • PR#42 · auth refactor    │   Implementing…  18m 42s  │
  │ HIGH                        │   Bash · Edit x2 · Read   │
  │  • Issue#78 · docs update   │   Cost: $0.14  Ctx: 42%   │
  └─────────────────────────────┴───────────────────────────┘
  ```
- Per-job display: status icon (⊛ running, ◌ queued, ✗ blocked, ✓ done) + title + elapsed + last 3 tools + cost/context meter
- Dispatch flow: `d` on a ready item → confirmation modal (model, worktree mode) → ENTER → spawn subprocess → immediately show agent detail view
- Review flow: agent sets status to `review` → TUI changes icon to ⏸ → user presses `y` (approve, agent resumes) or `x` (reject, prompted for feedback)
- Resume flow: blocked agent → agent detail view → `y` to resume from saved session ID (`--resume-from-session`)
- New domain type: `AgentSession { session_id, status, started_at, cost_usd, context_metrics, turns, activity_feed }`
- New TUI screens: `AgentDetail`, `AgentDispatchModal`, `AgentReviewModal`
- Key bindings: `d` dispatch, `y`/`n` approve/reject, `v` view transcript, `m` view diff, `i` interrupt, `Esc` back

### Agent Session Observability

- `.claude/session-stats/<session_id>.json` — cost (USD), duration, context % per session (written by Claude Code statusline hook)
- `.claude/projects/<project>/<session_id>.jsonl` — every message and tool call (Edit, Bash, Write, Read) in JSONL format
- Dashboard parses JSONL to render a live agent activity feed: tool calls as diff blocks, commands as code blocks, file writes highlighted
- Three distinct block types: diff blocks (−/+ prefixes, red/green), bash command blocks, file write blocks
- JSONL parsed line-by-line with a cap on total messages — handles large or still-growing session files without loading it all into memory
- Filter out system messages before rendering — only show user-facing tool use and responses
- Session stats surfaced in task detail view: model used, cost, duration, context remaining
- Context % rendered as a color-coded horizontal bar: green <75%, yellow 75–90%, red >90%
- If stats file is missing, show nothing — graceful degradation

### Agent Session Data Flow Reference

**File paths written by Claude Code:**
```
Session JSONL:   ~/.claude/projects/{URL_ENCODED_PROJECT_PATH}/{SESSION_UUID}.jsonl
Session stats:   ~/.claude/session-stats/{SESSION_UUID}.json
```

The project path is URL-encoded (slashes become `-`), e.g. `/Users/alice/Repos/hub` → `-Users-alice-Repos-hub`. The backend hardcodes the project path for its specific machine — a real implementation would read it from config.

**JSONL event schema** — every line is one of these types:

```json
// Session init metadata
{"type": "mode", "mode": "normal", "sessionId": "..."}
{"type": "permission-mode", "permissionMode": "bypassPermissions", "sessionId": "..."}

// File snapshot (for undo/redo, can be ignored for activity feed)
{"type": "file-history-snapshot", "messageId": "...", "snapshot": {...}}

// User turn (human message or tool result)
{
  "type": "user",
  "message": {"role": "user", "content": "..."},
  "uuid": "...", "timestamp": "2026-06-02T19:31:11.260Z",
  "cwd": "/path/to/project", "sessionId": "...", "version": "2.1.160", "gitBranch": "main"
}

// Assistant turn (text or tool_use)
{
  "type": "assistant",
  "message": {
    "role": "assistant",
    "content": [
      {"type": "tool_use", "id": "...", "name": "Read", "input": {"file_path": "..."}},
      {"type": "text", "text": "I'll analyze..."}
    ],
    "usage": {
      "input_tokens": 1,
      "cache_creation_input_tokens": 4020,
      "cache_read_input_tokens": 53318,
      "output_tokens": 170
    }
  },
  "uuid": "...", "timestamp": "..."
}
```

**Backend parsing logic** (key decisions):
- Line-by-line iteration, skips blank lines and JSON errors
- Only processes `type: "user"` and `type: "assistant"` — ignores `mode`, `permission-mode`, `file-history-snapshot`
- Filters system messages: skips if text starts with `<command-message>`, `Base directory for this skill:`, `<system-reminder>`
- Truncates to last 200 messages
- Extracts tool calls into typed blocks:
  - `Edit` → `{type: "edit", file_path, old_string, new_string}`
  - `Bash` → `{type: "bash", command}`
  - `Write` → `{type: "write", file_path, content}` (truncated to 50 lines, `truncated: true` flag set)
  - `Read` and all others → not extracted as special blocks (treated as text or omitted)

**Session stats file** written by a statusline hook at session end:
```json
{"model": "claude-opus-4-6", "cost_usd": 0.125, "duration_ms": 45000, "context_pct": 65.5}
```
Backend reads this and returns `{model, cost_usd, duration_s, context_pct}` alongside task data. Null values handled with `or 0` fallbacks.

**Frontend polling:** 10-second interval, plain HTTP GET (not SSE/WebSocket). Fetches `/api/agents` (task list + stats) and `/api/agents/{session_id}/messages` (parsed JSONL) independently. Uses `useRef` to track currently selected agent across the polling closure.

**For hub:** The session JSONL path template and event schema are stable Claude Code internals. The backend parsing approach (line-by-line, filter by type, extract tool blocks) is the right pattern. The 10s polling interval is reasonable for a live activity feed. The session stats file requires the statusline hook to be configured — hub already has this setup via its statusline configuration.

- JSONL event types to handle when parsing a live session stream:
  - `{"type":"system","subtype":"init"}` → "🎯 Session initialized (model)"
  - `{"type":"assistant","content":[{"type":"text"}]}` → "💬 Agent: …"
  - `{"type":"assistant","content":[{"type":"tool_use","name":"Edit"}]}` → "✏️ Edit /path (N lines changed)"
  - `{"type":"user","content":[{"type":"tool_result","is_error":false}]}` → "✓ succeeded"
  - `{"type":"user","content":[{"type":"tool_result","is_error":true}]}` → "✗ error: …"
  - `{"type":"result","total_cost_usd":0.287,"num_turns":8}` → "🏁 Done: turns=8 cost=$0.287"
- Activity feed icons: 🎯 init, 💬 message, ⚙️ tool call, ✓ success, ✗ error, ✏️ edit, 🔄 transition, 🏁 complete
- Agent detail view layout — metadata + metrics + scrollable activity feed + actions:
  ```
  ┌─ Issue#123 ooloth/hub: auth refactor ─ Agent Session ───────────────┐
  │ Issue: #123  Status: READY → IN_PROGRESS  Session: agent-2024-06-02 │
  │ Elapsed: 18m 42s  Cost: $0.287  Context: 68%  Turns: 8             │
  ├──────────────────────────────────────────────────────────────────────┤
  │ Activity Feed                                                        │
  │ 14:39  ✏️  Edit /src/auth.rs  (128 chars modified)                  │
  │ 14:38  ✓   Bash: cargo test — 42 passed (3.2s)                     │
  │ 14:36  💬  "Now implementing OAuth integration…"                    │
  │ 14:35  ⚙️  Read /docs/oauth-setup.md  (3.4k bytes)                 │
  ├──────────────────────────────────────────────────────────────────────┤
  │ v) transcript  m) diff  i) interrupt  y) approve  x) reject  Esc)  │
  └──────────────────────────────────────────────────────────────────────┘
  ```

### Human-Agent Collaboration

- Comment thread on each task: both human and agent write to `task_comments`; agent reads comments via CLI mid-task to pick up human feedback
- Task status is the handoff signal: agent sets `review`, human checks, human sets `done` or adds a comment and agent resumes
- Agent creates a markdown log file per task, links it back via `doc_links` — human-readable record of decisions and gotchas

### Workspace Management

- Two-tier directory structure: permanent read-only reference + ephemeral task working copies:
  ```
  workspaces/
  ├── _CANONICAL/              ← always on default branch, no venv, never edited
  │   └── repos/
  │       ├── repo-a/
  │       └── repo-b/
  └── TICKET-123/              ← one per task, deleted after task complete
      └── repos/
          ├── repo-a/          ← on feature branch, has .venv
          └── repo-b/          ← same feature branch, has .venv
  ```
- `_CANONICAL` is for reading only — agents are explicitly forbidden from making changes there
- One workspace per task; one workspace can span multiple repos (all on the same feature branch)
- Workspace ID = ticket/task key — implies branch name by default, overridable
- Split-install strategy for fast venv setup — avoids 12+ minute dependency resolution:
  ```
  Standard:      install -e .[extras]              → ~12 min (resolution + download)
  Split install:
    Step 1:      install --no-deps -r lockfile      → ~8s  (download only, no resolution)
    Step 2:      install --no-deps -e .[extras]     → ~2s  (editable install, deps satisfied)
    Total:                                          → ~10s
  ```
  Requires a pre-computed lockfile; falls back to full resolution if absent
- Three venv methods auto-detected from repo structure:
  - `pyproject` — single `.venv` at repo root (most repos)
  - `monorepo` — per-subproject `.venvs`, set up in parallel via thread pool
  - `none` — skip venv entirely
- `prepare` command resets a repo to default branch and creates a new feature branch — reuse a workspace across iterations without a fresh clone
- `commit-push` wraps the full push workflow: stage → commit (message from stdin) → optional squash → rebase on base branch → `--force-with-lease` push
- Cleanup is safe: check for unpushed commits (`git log @{u}..HEAD`) before deleting; exit non-zero if found — agent must confirm before proceeding
- Always-fresh per task: workspaces created fresh, worked in, deleted — no reuse across tasks

### Agent Skill Design

- Skills live in `.claude/skills/{skill-name}/SKILL.md` with YAML frontmatter — each is self-contained in its own directory
- Workflow skills prefixed with `wf-` — naming makes routing unambiguous
- Single entry-point skill (`/do-task TASK-XXXX`) routes to the right workflow by reading `task.type`
- Three-phase workflow structure with explicit checkpoints between phases — agent cannot skip a phase:
  ```
  Phase 1: SETUP       → read code, identify repos, create workspaces
  Phase 2: IMPLEMENT   → edit, test, quality checks, commit, push, wait for CI
  Phase 3: COMPLETE    → create PR, write log, update task status, clean up
  ```
- Checkpoint pattern: explicit STOP before each phase transition with a checklist the agent must verify:
  ```
  STOP. Before proceeding to Implement, verify:
  [ ] Workspace created at workspaces/{TASK-ID}/repos/{repo}/
  [ ] Feature branch checked out
  [ ] Requirements understood from code reading
  ```
- Task list created at workflow start with one item per phase segment — progress tracked visually, no steps skipped
- Escalation is explicit and immediate — comment + status update + stop work; not a retry loop:
  ```bash
  ctl tasks comment TASK-XXXX --author agent --content "{situation, attempts, why blocked}"
  ctl tasks update TASK-XXXX --status review --actor agent
  # STOP
  ```
- Escalate after 3 consecutive quality check failures — never loop indefinitely
- Tool failures are terminal: if a CLI command exits non-zero, report and escalate; never work around it
- Resume pattern: re-fetch task details (user may have added feedback), read prior log file, check PR comments, continue in existing workspace — don't recreate the workspace
- Leave workspace intact when abandoning mid-task — write findings to log, tag it on the task, escalate; workspace stays for resumption
- Agent log file per task with sections: Summary, Status, Changes Made, Key Code Patterns, PRs Created, Gotchas, Next Steps
- Dependency chain for multi-repo work is explicit and ordered: change lowest-level repo first, then consumers
- Timestamp before triggering async pipelines — used to find triggered downstream pipelines by filtering `--after {timestamp}` — the only reliable way to correlate across systems
- Record timestamps in the task list entry, not just in memory — survives session interruption
- Agents expected to update skill files with non-obvious findings — creates a feedback loop where agent experience improves the instruction set

### CI Pipeline Integration

- Two-step post-push CI workflow: wait for pipeline to appear (up to 60s), then block until complete — tools handle polling internally, agent just awaits exit code
- CLI output on success: `COMPLETED build=1247 result=SUCCESSFUL duration=12:34 url=...` (exit 0)
- CLI output on failure: `ERROR: Pipeline #1247 failed: result=FAILED duration=12:34 url=...` (exit 1) — exit code drives agent decision
- Pipeline log inspection subcommand hierarchy:
  ```
  pipeline-logs steps    → list steps in a build (name, state, result)
  pipeline-logs sections → parse a step's log into named script sections
  pipeline-logs get      → fetch log content, excluding runner/setup boilerplate
  pipeline-logs get --tail 50 → last N lines only
  ```
- Log parsing auto-excludes boilerplate (runner setup, teardown) — agent sees only actual script output
- Pipeline failure diagnosis loop: get failed step → get sections → get log → analyze → fix locally → re-run quality checks → commit → push → repeat; escalate after 3 failures
- Find triggered downstream pipelines by `--after {timestamp}` and/or `--trigger {pattern}` — timestamp recorded before triggering is the correlation key
- Custom pipeline trigger with `--wait` flag blocks until complete — no separate wait call needed
- CI failure diagnosis for GitHub Actions: `gh run view --log-failed` + grep for `error[` / `^Error` patterns — same pattern, different provider
- "Find last success, diff to first failure" pattern for regression hunting: list recent runs → find boundary → compare changed files between the two commits
- No real-time CI badge in the dashboard — CI monitoring is agent-driven via blocking CLI tools, not ambient display
