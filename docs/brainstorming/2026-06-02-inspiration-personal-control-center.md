# Inspiration: a personal control-center project

Raw itemization of ideas from a personal control-center project with a web dashboard + agent
CLI + SQLite backend. Goal: apply the best of these to hub's TUI. No filter — big and small.

Agent task management patterns are in a separate doc (`2026-06-02-agent-task-management.md`).
This doc covers everything else.

---

## Data Model

- Comma-separated fields (`jira_tickets`, `pr_links`, `doc_links`) for 1:N resource links — deduplicated on write, split on read, no join table needed for small collections
- Soft deletes via status field (`archived`, `deleted`) — never hard-delete records
- Numbered SQL migration files applied automatically on connection, tracked in a `schema_version` table
- WAL mode + foreign keys enabled on every SQLite connection
- Immutable activity log table: every state change recorded with actor, action, JSON detail, timestamp — full audit trail without touching the main record

## Caching

- File-based cache with TTL: `{"timestamp": <unix>, "data": <payload>}` — lazy expiration on read, no cleanup needed
- Each data source tracks `is_refreshing`, `last_refreshed`, `last_error` — surfaces staleness and errors independently
- `last-refreshed.json` persists refresh timestamps across restarts so the app doesn't re-fetch everything on startup
- Cache invalidation on manual refresh, automatic on TTL expiry
- No external cache service — everything in local files

## Live Updates

- `DataSourceManager`: registers async fetch functions with per-source configurable intervals
- Each source emits events (`DataSourceEvent`) to SSE subscribers on refresh — frontend knows exactly which source updated
- Polling beats WebSockets for a single-machine dashboard: simpler, no connection management, survives restarts
- Per-source refresh intervals tuned to signal volatility (agent dispatch: 60s; PR/CI: 300s; alerts: varies)
- Manual "refresh" button invalidates cache and triggers immediate re-fetch

## Dashboard Layout

- Ranked signal list alongside a jobs/tasks panel — two concerns visible simultaneously without switching screens
- Task detail view shows metadata, linked resources, comment thread, activity log, and agent session data all on one screen
- Type-specific UI: flux workflow tasks show related error runs inline; implement tasks show PR status
- Clickable linked resources (tickets, PRs, doc paths) navigate directly to the relevant item
- Status lozenges / badges inline in lists — status scannable without opening detail

## CLI Design (agent-friendly)

- Structured single-line output for machine parsing: `TASK_CREATED TASK-0042`, `DISPATCHED TASK-0042 <id>`, `NO_READY_TASKS`, `AT_CAPACITY`, `RESUMED TASK-0042 <id>`
- Errors to stderr, exit code 1, actionable message: `ERROR: task TASK-0042 status is 'in-progress', expected 'ready'`
- Dry-run mode: `DRY_RUN TASK-0042` — same output shape, no side effects
- Commands designed for subprocess consumption by dashboard backend and agent dispatch loop, not just human use
- Dashboard calls CLI via `asyncio.create_subprocess_exec()` and parses stdout — CLI is the single source of truth for business logic

## Agent Session Observability

- `.claude/session-stats/<session_id>.json` — cost (USD), duration, context % per session (written by Claude Code statusline hook)
- `.claude/projects/<project>/<session_id>.jsonl` — every message and tool call (Edit, Bash, Write, Read) in JSONL format
- Dashboard parses JSONL to render a live agent activity feed: tool calls as diff blocks, commands as code blocks, file writes highlighted
- Session stats surfaced in task detail view: model used, cost, duration, context remaining
- If stats file is missing, show nothing — graceful degradation

## Human-Agent Collaboration

- Comment thread on each task: both human and agent write to `task_comments`, agent reads comments via CLI mid-task
- Agent creates a markdown log file per task, links it back via `doc_links` — human-readable record of decisions
- Task status is the handoff signal: agent sets `review`, human checks, human sets `done` or adds a comment
- `RESUMED` output code when re-dispatching a task that already has a session — no new session created

## Agent Dispatch

- Deterministic session IDs via `uuid5(namespace, task_key)` — same task always maps to same Claude Code session, enables resume without lookup
- Capacity management: count `status='in-progress' AND agent_session_id IS NOT NULL` as a semaphore, respect `--max-sessions`
- Dispatch loop is safe to run on a timer — atomic claim prevents double-dispatch
- `NO_READY_TASKS` and `AT_CAPACITY` are expected non-error states, not failures
- Dispatch supports multiple launch targets (different terminal apps) with graceful fallback

## Testing

- Isolated temp SQLite DB per test — no mocking of internal logic, real migrations run
- Test both success and precondition-failure paths (e.g. claiming a task that isn't `ready`)
- Test deduplication: adding a duplicate ticket/PR/doc → same state, no duplicate
- CLI tested via `CliRunner` equivalent — invoke handler directly with test DB

## Task Type System

- Task types (`implement`, `debug`, `general`) guide which skill/prompt the agent uses at dispatch
- Type-specific UI in the dashboard — different data shown for different task types
- Could map to investigation types in hub (e.g. `ci-failure` auto-loads Codefresh context)

---

## Layout and Navigation

- Three-pane layout: compact list (fixed width) + scrollable detail pane + scrollable chat/activity pane — each scrolls independently
- Segmented control / tab switching within a page encodes selected tab into URL (or TUI state) — preserves context on back-navigation
- Detail panel opens alongside the list (split view), not replacing it — list stays visible for comparison and quick jumping
- Breadcrumbs above detail titles so the user knows exactly where they are and how to get back
- Nav sidebar badges (count of urgent items per section) update independently on their own polling interval
- Force-refresh of badges via event dispatch — any screen can trigger a global badge refresh without knowing about the nav component

## List View Details

- Sort by most recent update by default — surfaces items that need attention, not just newest-created
- Color-coded age badges inline in list rows: green ≤2 days, yellow ≤6 days, red >6 days — staleness visible without opening detail
- Item count in section/column headers: "Needs review (5)" — scan total load at a glance
- Category grouping without kanban columns: tabs for "Mine / Needs review / Approved / Team" rather than drag-and-drop swim lanes
- Row striping + highlight-on-hover — reduces eye travel across wide rows
- Snooze mechanism: hide items from urgency counts and badge totals until new activity occurs; auto-unsnooze when the item updates
- "New since snooze" marker: highlight activity that arrived after the item was snoozed — surfaces only the delta
- Fixed-width columns with explicit sizing — consistent layout even with variable-length content

## Detail View

- Linked resources grouped by type (tickets, PRs, docs) with icons and live-fetched state badges (PR open/merged/declined)
- PR status badge fetched dynamically per linked URL — doesn't rely on stored state going stale
- Activity timeline with type-specific icons (comment = message, approval = check, changes-requested = warning, commit push = git icon)
- Context % rendered as a color-coded horizontal bar: green <75%, yellow 75–90%, red >90% — agents going critical is immediately obvious
- Formatted elapsed time: "2m 30s", "45s", "2d ago", "3h ago" — not raw timestamps
- Monospace font for technical values: session IDs, file paths, command strings, identifiers
- Inline URL detection in description text: split on `https?://` regex and render as clickable links, not just plain text
- Truncation indicator when content is cut off — never silently hide data

## Rendering Agent Activity

- Three distinct block types for agent tool calls: diff blocks (old/new with −/+ prefixes), bash command blocks, file write blocks
- Diff blocks show `old_string` vs `new_string` as two sections with colored prefixes (red for removed, green for added)
- Cap JSONL parsing at N messages — safe, incremental, never loads a huge session into memory all at once
- Filter out system messages from JSONL before rendering — only show user-facing tool use and responses
- JSONL parsing is line-by-line, not full-file parse — handles large or still-growing session files

## Data Loading and Refresh

- `DataSourceManager` with per-source async locks — sources refresh independently and in parallel without stomping each other
- Per-group serialization lock when sources share state — prevents thundering herd on related sources without blocking unrelated ones
- Startup skips re-fetching sources refreshed recently — reads `last-refreshed.json`, only fetches stale ones
- Refresh state persisted to disk (`data-sources.json`) — re-launching the service doesn't lose interval config or last-refresh times
- SSE event stream for refresh events: each source emits `{type, key, timestamp, error}` — consumers know exactly which source changed
- SSE queue drops messages if full (non-blocking `put_nowait`) — a slow consumer never blocks the refresh loop
- Multi-TTL caching: short TTL (300s) for current-state data, long TTL (7200s) for slower scans — different signals decay at different rates
- Large file size guard on cache reads (`max_bytes=500_000`) — never accidentally load a multi-MB cache file into memory
- `LoadingOverlay` on in-place refresh — content stays visible but dimmed; no unmount/remount flicker

## Empty and Loading States

- Placeholder non-breaking space (` `) when no data — maintains vertical alignment without layout shift
- Dimmed color for "no items" text — visually distinct from real data, not an error state
- Spinner during initial load; smaller spinner during incremental re-fetch — communicates different states differently
- Stale data badge: "Stale — last run 2024-12-15" in yellow when data exceeds age threshold — never silently serve old data

## Forms and Interactions

- Two-stage confirm for destructive actions: first click shows "Confirm delete", second click executes — prevents accidents
- Dirty flag on edit forms — save button disabled until a change is made
- Optimistic updates: update local state immediately on drag/reorder, confirm with server asynchronously, roll back on next fetch if server rejected
- Autocomplete for category/type fields using existing values — reduces typos, surfaces valid options without a closed enum
- Copy-to-clipboard button with feedback: icon changes from copy to checkmark, tooltip shows "Copied!" — micro-confirmation
- Drag-and-drop with drag-counter pattern: `dragCounter++/--` to only highlight drop target when actually over it, not just over a child element

## Status and Health Indicators

- Semantic color map for statuses: backlog=indigo, ready=blue, in-progress=cyan, blocked=red, review=orange, done=green — consistent everywhere, never ad hoc
- KPI summary cards: large number + small label + icon watermark at low opacity in background — high-information density, scannable at a glance
- Timeline chart for trend visibility: 30-day dual-axis chart (failures count + duration) — detects degradation that point-in-time status misses
- Per-item health computed from multiple signals, not a single field — e.g. PR urgency from age + review status + CI status combined

## Database and Schema

- SQLite `GENERATED ALWAYS AS (...) STORED` for derived keys — `TASK-0042` computed at insert from integer ID, indexed, immutable, no application logic needed
- Dataclass + `from_row()` / `to_dict()` pattern: `TABLE_COLUMNS` tuple, typed row mapping, JSON serialization — type safety without an ORM
- Schema validation on startup: check for missing indexes, orphaned records, inconsistencies — warn to stderr before accepting traffic
- Flexible task reference resolution: accept both integer IDs and `TASK-XXXX` format — user-friendly references, simple DB

## Background Jobs and Scheduling

- Per-source async lock for independent refreshes + per-group lock for sources sharing state — parallelism where safe, serialization where needed
- Subprocess calls always use timeout + returncode check — no hanging calls, no silent failures
- Errors in one data source don't block others — each source handles its own errors and marks itself errored independently
- Retry not baked in at the scheduler level — sources that fail just show `last_error`; user triggers manual refresh

## CLI Architecture

- CLI is the single source of truth for business logic — dashboard backend calls CLI via subprocess, never reimplements the logic
- `kwc` commands accept both human-friendly (TASK-0042) and machine-friendly (integer ID) references
- Parallel repo/API discovery with bounded thread pool (20 workers) — fast enough without overwhelming rate limits
- API field pruning via `?fields=a,b,c` query param — fetch only what's needed, reduce payload and pagination
- Token files in `~/.service/token` with clear error messages if missing: exact path + link to generate token — no silent auth failures
- Environment variable for config dir with fallback to relative path from module root — portable without being rigid
- Agent instructions explicitly state "use kwc, don't call APIs directly, don't attempt workarounds if kwc fails" — prevents agents from diverging from canonical tool behavior

## Agent Instruction Design

- Workspace isolation: canonical repos are read-only reference copies on default branch; working copies are per-task directories — agents can't accidentally mutate the reference
- Agents expected to update skill files with non-obvious findings from their sessions — creates a feedback loop where agent experience improves the instruction set
- Skills referenced by name in CLAUDE.md, not inlined — agents load the relevant skill for the task type, keeping instructions focused

---

## Monitoring and CI/Pipeline UI

- "ALL PASS" vs large red failure count as the primary status display — no ambiguity, no scanning required
- Pass/fail result badge as a single reusable component: green=PASSED, red=FAILED, gray=unknown — used everywhere consistently
- Color-coded dots at each chart data point (green=pass, red=fail) in addition to line color — scan for red is instant
- Dual-axis line chart: failures count on left Y-axis, duration on right Y-axis — spots performance regressions alongside failure trends in one view
- Reference line at average duration on runtime chart — immediately shows whether a run is an outlier
- Pass/fail ratio as a pie chart paired with a runtime-over-time line chart, side by side — two complementary views of the same history
- Variable lookback window: if latest data is stale, extend the window beyond 30 days; clamp to `[1, 30]` days — adapts to actual data freshness
- Stale data warning badge: yellow "Stale — last run YYYY-MM-DD" when data exceeds 24h age — never silently serve old data
- Dual-state tabs with counts: "Current (7)" and "Resolved (23)" — filter unresolved vs resolved without separate pages, count always visible
- Sortable paginated run table (PAGE_SIZE=15) with date, run #, result, env, test counts, duration — limits render cost while showing depth
- Run detail panel in a sticky sidebar (1/3 width): selecting a run from the list (2/3 width) opens detail alongside without leaving the list
- Test history drill-down: pipeline → individual test → 30-day history of that specific test across runs
- Build/run number as a clickable link to the external pipeline — `#123` not a UUID, human-readable
- Structured error metadata shown before the raw error dump: step type, workflow name, project code, timestamps — triage without reading the full error
- Monospace code block for error text with `maxHeight` and scroll — preserves whitespace, searchable, never wraps awkwardly
- Error line-clamping in table cells (3 lines max) — shows enough to recognize the error without blowing out the row height
- "Task already created" indicator: if an error run has a linked task key, show it as a badge; otherwise show a "create task" action — closes the loop between monitoring and work tracking
- Annotation columns added to monitoring records post-hoc (`resolved`, `resolution` enum, `notes`, `task_key`) — triage state layered onto raw run data without schema redesign
- Resolution category as a fixed enum (8 options: "User Error", "Transient", "Known Error", "Ignore", etc.) — fast triage, consistent vocabulary
- Freeform notes field with save-on-blur, not save-on-keystroke — avoids thundering herd of DB writes
- Resolved checkbox moves item to "Resolved" tab without deleting — soft-resolve, always undoable
- Daily failure count aggregation query: `GROUP BY DATE(started)` → `{date, count}` array — feeds trend charts and summary counts from one query
- Deep-link support: URL query param `?run=ID` auto-selects that run in the detail panel on page load — linkable state without a router
- Environment-specific external dashboard URLs (PROD vs PRE vs DEV) — prevents linking to wrong environment
- Stepper component for multi-step pipeline state — shows which step is the current blocker visually

## Home / Overview Screen and Cross-Signal Aggregation

- Fixed-width status cards (one per signal category) in a flex-wrap grid — add/remove signals by toggling visibility, layout adapts automatically
- Each card: icon watermark + title + large primary count + colored status text + secondary detail line — three-row structure, consistent across all cards
- Primary count color codes urgency (red = action, yellow = attention, green = clear); status text reinforces it in words — redundant encoding for colorblind users
- Secondary detail line ("2 drafts", "3 ready") gives context without competing with the primary number
- Reserve space with a non-breaking-space placeholder when secondary detail is empty — no layout shift when data loads
- One summary endpoint `/api/status` returns all badge counts in parallel — prevents N separate fetches for the sidebar
- Sidebar nav badges computed from the same summary data: red = action required, orange = attention, hidden = clear — trains users to scan red first
- Badge refresh on a separate timer (every 5 min) independent of page content refresh
- Event-driven badge refresh: any screen can fire a "refresh-badges" event after an action; the badge hook re-fetches — sidebar stays in sync without a full reload
- Fetch one full dataset per source, slice it in the consumer — no separate endpoints for "mine" vs "team" vs "needs review"; filter client-side
- Multi-source deduplication with precedence: if a PR appears in "approved pending", remove it from "needs review" — never show the same item in two urgency categories
- Snooze/suppress per item: hide from badge counts until new activity; auto-unsnooze when the item updates — keeps the dashboard clean without losing alerts
- "Also check" cross-signal grouping: combine two related signals (e.g., ArgoCD out-of-sync + Flux behind) into one card — communicates "fix these together"
- Time-horizon bucketing for todos: "today" vs "this week" vs "backlog" — urgency encoded in the category, not just a priority field
- Per-source refresh intervals tuned to volatility: critical sources (PRs, CI) every 5 min; slower sources (tickets, tasks) every 30 min
- If one source errors, show "!" on that card only — other cards unaffected, error is localized
- Pending counter for coordinated multi-source refresh: each source increments on start, decrements on finish; spinner clears only when counter hits zero
- Show "?" as placeholder while data loads for the first time — no layout shift, communicates "coming soon" vs "none"

## Keyboard and Interaction Patterns (TUI Translations)

- Escape closes the current modal/drawer — implement as highest-priority key in the event loop
- Enter submits the focused form field without requiring a "Save" button click
- Inline editing: press a key to switch a display field to an edit field with autofocus; Enter saves, Escape cancels — no separate modal needed for single-field edits
- Two-stage confirm for destructive actions: first keypress shows "Confirm?" prompt, second keypress executes — prevents accidents
- Drag-and-drop status change in the web app → in TUI: arrow keys to move between status columns, Enter to confirm move
- Search field clears on Escape — consistent with modal-close behavior, muscle memory transfers
- Disabled state for actions that require prior input — visually distinct, still focusable
- Status dropdown: arrow keys navigate options, Enter selects — standard behavior worth preserving in TUI popups
- Type-to-filter in autocomplete fields — maps directly to TUI filter-as-you-type
- Selected item highlighted with border/background color change — unambiguous current position in list
- "?" overlay showing available shortcuts — the web app doesn't have this, but it's the obvious missing piece; TUI should
- Footer hint bar showing context-sensitive keys for the current screen — reduces discoverability gap
- Nested modals/popups each handle their own Escape: last-opened closes first — clean layering without global state
- No global application hotkeys in the web app (it's mouse-first) — TUI inverts this: every action should have a key, mouse is optional

---

## Tickets / Issues List

- Hierarchical categorization by context: "mine" (assigned to me), "unassigned sprint" (team responsibility), "recent" (any assignee, last 14d) — each is a separate query, merged and deduplicated:
  ```
  Assigned to me (5)
  ├─ DTS-123  Fix auth bug      [In Progress]  1d
  ├─ DTS-124  Update docs       [To Do]        3d
  └─ (3 more)

  Other recent updates (12)      ← deduped: already-mine tickets removed
  ├─ DWO-456  Setup CI          [To Do]        2d
  └─ (11 more)
  ```
- Section headers always show item count in parentheses — scope visible at a glance without counting rows
- Unassigned tickets highlighted in bold red in the list — "needs an owner" is a first-class signal
- Deduplication with precedence: if a ticket appears in "mine", remove it from "recent" — never show the same item twice across sections
- Lean column set: Key, Summary, Status, Updated, Comments, Type, Priority, Assignee — nothing more
- Comment count as activity proxy: 0 = fresh/unblocked, 5+ = active discussion or complex
- Age-based color on timestamp column: green ≤2d, yellow 3–6d, red ≥7d — staleness visible without reading the date
- Status badge color: green=Done, blue=In Progress, gray=To Do — reserved red for alerts only, not normal statuses
- Summary column wraps to 2 lines max — enough context without horizontal scroll
- Two refresh modes: cached overview (fast, for status cards) vs force-refresh endpoint (slow, for full page) — same data, different freshness guarantees
- Parallel JQL queries with bounded thread pool — all board sections fetch concurrently, each independently handles errors
- ADF → plain text extraction for ticket descriptions: strip tags, convert `<br>` to newlines, preserve links as `[text](url)`
- Section ordering by urgency: "mine" first, then "unassigned", then "completed sprint", then "recent FYI" — trains the eye to scan top-down

## PR List and Detail

- Four sections in the PR list, each independently scrollable:
  ```
  Needs my review (3)    My PRs (2)    Approved, awaiting merge (1)    Team requests (4)
  ```
- Per-row columns: Repo, PR#, Title, Comments, Updated, Author, Created, Snooze toggle — fixed widths, ellipsis on overflow
- Draft and Snoozed badges inline next to PR number, not in the title column:
  ```
  #123 [Draft]     #456 [Snoozed]
  ```
- Snoozed rows at 45% opacity — present but visually suppressed
- Age badge coloring on both Updated and Created columns: green ≤2d, yellow ≤6d, red ≥7d
- Snooze stores `updated_on`, `comments`, and `source_hash` at snooze time; auto-unsnoozes if any of those change on next fetch — tracks the delta, not just time
- Right-side detail drawer (size: large) opens on row click, list stays visible on left:
  ```
  ┌──────────────────────────┬──────────────────────────────────┐
  │ Needs my review          │ flux-models          (repo)       │
  │ ● #123  2d  alice  [!]  │ #123 | Fix auth token bug         │
  │ ● #124  5d  bob         │ Author: @alice  [Open in GitHub]  │
  │                          │ ─────────────────────────────     │
  │ My PRs                   │ Recent Activity                   │
  │ ● #125  1d  [open]      │                                   │
  │ ● #126  3d  [draft]     │ ✓ bob approved          3h ago    │
  │                          │ ✗ charlie req. changes  1d ago    │
  │                          │ ↑ alice pushed abc1234  2h ago    │
  │                          │ 💬 "looks good overall" 4h ago   │
  └──────────────────────────┴──────────────────────────────────┘
  ```
- Activity timeline in drawer: icon + colored badge per event type (comment=blue message, approval=green check, changes-requested=orange warning, push=gray git-commit)
- Activity truncated: comments to 300 chars, commit messages to first line / 200 chars — never fills the pane with a wall of text
- Post-snooze activity highlighted with violet left border + light violet background:
  ```
  │ ← violet border
  │ [New] ✓ alice approved  1h ago    ← violet "New" badge
  │   "All good, merging tomorrow"
  ```
  Footnote: "Activity above marked 'New' occurred after snooze"
- Two-line drawer header: repo name (small, dimmed) above PR title (bold) — repo context without eating column width
- Update events synthesized into human-readable summaries: "Pushed commit abc1234", "Marked as draft", "Reviewers updated", "Title updated: new title" — not raw API event objects
- Parallel source aggregation: Bitbucket + GitHub merged by URL, approved_pending takes precedence over needs_review if same PR appears in both — no duplicates, no ghost PRs
- Discover mode for PR scanning: scan all repos updated in last 14d, union with whitelist, subtract blacklist — cached separately at longer TTL than core repos

## Calendar / Schedule Integration

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
  │ ...                                 │
  │ 11:00 ── now ────────────────────── │  ← red dot + line
  │ 11:30 ┌────────────────────────┐    │
  │       │ Design Review          │    │
  │       │ 11:30 – 13:00          │    │
  │       └────────────────────────┘    │
  │ ...                                 │
  │ 19:00                               │
  └──────────────────────────────────────┘
  ```
- All-day events in a separate section above the hour strip — don't clutter the timed grid
- "Now" indicator (red dot + horizontal line) auto-scrolls into view when opening today's calendar
- Past events fade to 50% opacity and gray — the past doesn't compete with what's coming
- Minimum event height enforced (prevents unreadably thin blocks for short meetings)
- Events clipped to the 8–19 window — anything outside is silently excluded
- Event detail modal on click: title, time, organizer, location, Google Meet URL (if found in description/location via regex), cleaned description
- Google Meet URL extraction via regex on location and description fields — one-click join from the dashboard
- All-day and timed events separated at the data level (`all_day: bool` flag) — different rendering, not just different fields
- Event exclusion via configurable pattern list (`DASHBOARD_CALENDAR_EXCLUDE="lunch,personal,blocked time"`) — hide low-signal events without deleting them
- Cancelled events (`STATUS=CANCELLED`) filtered out server-side
- Two-layer cache: frontend 5min in-memory + backend 45min file-based — two misses required before a real fetch
- Per-date fetching (not range-based): `GET /api/calendar?target_date=2024-06-02` — one cache entry per date, no range bloat
- Timezone-aware parsing server-side, display as HH:MM strings — no client-side timezone math
- Recurring event expansion (RRULE) done server-side — frontend sees a flat list of single-day occurrences
- HTML cleanup for Google Calendar descriptions: `<br>` → newline, strip tags, remove Google's separator string
- **Gap worth filling**: calendar and tasks are completely separate silos — no cross-reference between meeting load and task commitments. A TUI could show task deadlines relative to calendar, warn about over-scheduling, or surface free blocks for deep work

## Configuration Architecture

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
  Static (code):           API_TO_REPO = {"chemist-api": "flux-models", ...}
  Mutable (JSON file):     data/config/data-sources.json → {"intervals": {"prs": 600}}
  ```
- Data source refresh intervals have `default_interval` (immutable, from registration) and `interval` (mutable, from config file) — user tunes at runtime, survives restarts
- Config flow:
  ```
  hub.toml / code constants
      ↓ env vars (DASHBOARD_*)
      ↓ fallback to repo-relative paths
      ↓ database (mutable repo metadata)
      ↓ JSON config files (intervals, last-refreshed timestamps)
      ↓ file cache (per-key .json files)
  ```
- Schema validation at startup returns warnings (not exceptions) — loud but doesn't crash; operator gets a chance to diagnose
- Read-only vs read-write DB handles documented explicitly — dashboard is a consumer, tools layer owns migrations
- Snooze state and other ephemeral mutable state in JSON sidecar files (`data/pr-snooze.json`) — not in DB (too ephemeral), but survives restarts
- Cache invalidation via file deletion — idempotent, decoupled, debuggable by looking at the filesystem
- Group locking for related data sources: sources in the same group share a lock so they don't fetch concurrently:
  ```
  group="bc-compound-design" → flux-config + workflow-config refresh serialize
  ```
- Adding a new data source: write fetch function, register with `manager.register(key, label, fn, default_interval)`, done — config auto-persisted, cache auto-managed, SSE auto-emitted

---

## Todo Board

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
- Item count badge in every column header — scope visible without counting
- 4-level priority system communicated via icon + card border color:
  ```
  low    → gray  ↓  (no border)
  normal → blue  =  (no border, default)
  high   → orange ↑ (colored left border)
  urgent → red   !  (colored left border, stronger)
  ```
- Priority picker in edit modal: 4 icon buttons, selected at full opacity, others dimmed — not a dropdown
- Card shows only: title (2 lines, bold), description (2 lines, dimmed, only if non-empty), type badge — everything else is in the detail modal
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
- Archive tab is read-only with a "Clear Archive" bulk delete — requires confirmation, irreversible, removes all `archived` items
- Todos and tasks are separate entities in separate databases with separate status vocabularies:
  - Todos: `backlog / next-week / this-week / today / done / monitor / workstreams / archived` — lightweight, personal
  - Tasks: `backlog / ready / in-progress / blocked / review / done / archived` — tracked, linked to tickets/PRs/docs
- Drag-and-drop uses a counter pattern to handle nested elements without flicker: `dragCounter++` on enter, `dragCounter--` on leave, apply highlight only when counter > 0
- Optimistic update on drag: move card in local state immediately, then sync with API, then full refresh — no waiting for the server
- Edit modal has dirty tracking: Save button disabled until a field changes AND name is non-empty
- Tab state encoded in URL search params (`?tab=backlog`) — back button navigation works, state survives refresh

## Repo Management

- Three support tiers derived at runtime from two boolean fields (never stored as a tier column):
  ```
  venv_configured=true  AND tests_configured=true  → Fully Supported  (green)
  venv_configured=true  OR  tests_configured=true  → Tracked          (yellow)
  neither                                           → Discovered       (gray)
  ```
- Repo schema: `name, origin, venv (enum), venv_configured, tests_configured, custom_test_handling, notes` — capability bits, not status
- `venv` field is an enum routing hint, not a boolean: `pyproject` (single venv at root), `monorepo` (per-subproject venvs), `none` (skip) — determines workspace setup strategy
- `notes` field captures operational blockers: "needs libomp", "private pip index unreachable", "macOS-only, no Linux wheels" — explains why `venv_configured=false` despite being a known repo
- Rows with notes shown in warning color — the note is a signal, not metadata
- Edit modal shows derived "Fully Supported" badge that recomputes live as user toggles the two flags — immediate visual feedback:
  ```
  [✓] Venv Configured
  [✗] Tests Configured
  ──────────────────────
  Status: TRACKED  (not fully supported yet)
  ```
- Two separate repo views with different concerns:
  - Metadata repos — all tracked repos, configurable, filterable, stable
  - Git repos — subset with rich commit history, tags, release management; expensive to fetch, cached aggressively
- Git commit graph rendered with Unicode box drawing: main lane vs branch lanes in different colors, release tags as larger nodes, "untagged commits since last release" highlighted in orange as a signal to cut a release
- Origin resolution tries DB lookup first, then falls back to platform conventions (Bitbucket org, then GitHub org) — warns if origin was guessed
- Host extracted from `origin` field for filtering and badging: `github.com` → GitHub, `bitbucket.org` → Bitbucket
- Batch filter → action pattern: filter list, select multiple, apply update to all selected — useful for "mark all pyproject repos as tests_configured"

## Settings / Data Source Management

- Settings as a dedicated screen with sidebar nav: General, Data Sources, Repositories — each a full subpage, not a modal
- Data source list as a table, one row per source:
  ```
  ┌─ Source ──────────┬─ Status ──┬─ Last Refreshed ─┬─ Interval ─┬─ Data ─┬─ ↻ ─┐
  │ Pull Requests      │ ✓ OK      │ 3m ago           │ 5m         │ 👁     │  ↻  │
  │ Flux Errors        │ ✗ Error   │ 45m ago          │ 10m        │ —      │  ↻  │
  │ E2E Tests          │ 🔄 ...    │ just now         │ 5m         │ 👁     │  ↻  │
  └───────────────────┴───────────┴──────────────────┴────────────┴────────┴──────┘
  ```
- Status badge states: ✓ OK (green), ✗ Error (red, tooltip shows message), 🔄 Refreshing (blue), ⏳ Never (gray)
- Error badge is hoverable — shows the actual error message inline; don't bury it in a modal
- Interval column is click-to-edit inline — number input appears in place, Enter to save, Escape to cancel; no modal needed
- Interval = 0 means disabled — unifies "pause" and "configure rate" into one field
- Shows both current interval and default: "10m (default: 5m)" when customized — never hides configuration drift
- Data preview button (👁) opens a modal showing the raw cached JSON — disabled until data exists and is under 500KB; tooltip explains why when disabled
- Immediate saves — no "Save" button anywhere; changes take effect on Enter/confirm
- "Last refreshed" column updates via a 30-second background timer without re-fetching — relative times stay fresh locally
- Lazy SSE connection: wait 3 seconds after page load before opening the event stream — lets initial data fetches complete first
- Separate `last-refreshed.json` file persists refresh timestamps across restarts — "3m ago" survives a process restart
- Startup strategy: refresh only sources that are stale (older than their interval), serve cached data for the rest — don't refetch everything on launch
- Loading spinner with 3-second timeout: if no data arrives, show "No sources registered" — never spin forever

## Workspace Management

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
- `_CANONICAL` is for reading (browsing code, URL resolution) — agents are explicitly forbidden from making changes there
- One workspace per task; one workspace can span multiple repos (all on the same feature branch)
- Workspace ID = ticket/task key (e.g., `TICKET-123`) — implies branch name by default, overridable with `--branch`
- Split-install strategy for fast venv setup — avoids 12+ minute dependency resolution:
  ```
  Standard:      uv pip install -e .[extras]          → ~12 min (resolution + download)
  Split install:
    Step 1:      uv pip install --no-deps -r lockfile  → ~8s  (download only, no resolution)
    Step 2:      uv pip install --no-deps -e .[extras]  → ~2s  (editable install, deps satisfied)
    Total:                                              → ~10s
  ```
  Requires a pre-computed lockfile (`uv.lock`, `pinned-versions.txt`, or `lockfiles/3.11/lockfile.txt`); falls back to full resolution if absent
- Three venv methods auto-detected from repo structure:
  - `pyproject` — single `.venv` at repo root (most repos)
  - `monorepo` — per-subproject `.venvs`, set up in parallel via thread pool
  - `none` — skip venv entirely (non-Python or special repos)
- Python version read from `.python-version` file (pyenv format), falls back to `3.11`
- Cleanup is safe: check for unpushed commits before deleting (`git log @{u}..HEAD`); exit with error if any found — agent must confirm before proceeding
- `prepare` command resets a repo to default branch and creates a new feature branch — used to reuse a workspace across multiple iterations without a fresh clone
- `commit-push` wraps the full push workflow in one command: stage specified files → commit (message from stdin) → optional squash → rebase on base branch → `--force-with-lease` push
- Always-fresh per task: workspaces are created fresh, worked in, then deleted — no reuse across tasks
- Workspace-to-task association: task record stores `jira_tickets` field; workspace path derived from it; agent uses this to know where to work
- Safety on cleanup: if workspace contains unpushed commits, emit `UNPUSHED_CHANGES` and exit non-zero — agent must get confirmation before deleting
