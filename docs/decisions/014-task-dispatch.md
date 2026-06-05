# 014 — Task dispatch: workspaces, prompt injection, completion detection, data home

## Context

[Decision 013](013-task-session-model.md) established the one-task-one-session model,
deterministic uuid5 session IDs, and JSONL-driven status inference. It did not specify
*how* dispatch actually happens: where the agent runs, how it receives its instructions,
how the TUI detects when it is done or stuck, or where hub's system data lives. This
decision resolves those questions as a single coherent system (the choices interact).

This decision was shaped by a session-pricing constraint that was not present when 013
was written: as of June 2026, `claude -p` (headless/API mode) is metered at API rates
and is not included in the subscription. **All agent sessions must use interactive
`claude` (no `-p`)** to stay within subscription pricing. This rules out the `result`
event in the JSONL that 013 assumed would be the completion signal.

---

## Decisions

### Task workspaces live in `~/.hub/workspaces/`

Each task gets a persistent workspace directory for its agent session:

```
~/.hub/
  repos/                       ← bare git clones + managed worktrees (existing)
    hub/
      main/                    ← default-branch worktree
      pr-123/                  ← PR investigation worktrees
  workspaces/                  ← task agent workspaces (new)
    TASK-0001/
      hub/                     ← git worktree of hub repo on branch agent/TASK-0001
    TASK-0042/
      hub/                     ← git worktree for another task
  agent-session-logs/          ← agent completion reports (new)
  transcripts/                 ← existing session transcripts
  hub.db                       ← database (see below)
```

Task workspaces are **separate from** `repos/`. The `repos/` subtree is managed
by the fetch loop and `clean_merged_worktrees`; mixing task worktrees in there would
expose them to that loop's cleanup logic. Task workspace cleanup is independently
controlled.

The workspace for a single-repo task is `~/.hub/workspaces/TASK-XXXX/<project>/`.
This layout is forward-compatible with multi-repo workspaces (future): adding a second
repo is just adding another subdirectory under `TASK-XXXX/`.

**Why `~/.hub/` and not the project directory**: task worktrees must not live inside
the human's working copy. They are system-managed, not source-controlled alongside hub
code. `~/.hub/` is already hub's system home (repos, transcripts); workspaces and logs
belong there too.

### Worktree strategy: persistent named branch, not ephemeral

Investigation sessions use ephemeral worktrees (detached HEAD, cleaned up on exit).
Task sessions use **persistent named worktrees on a feature branch**:

- Branch: `agent/TASK-XXXX` (checked out fresh from trunk at first dispatch)
- Worktree: `~/.hub/workspaces/TASK-XXXX/<project>/`
- Persists across session termination — the agent resumes in the same worktree with
  its prior commits intact
- Cleaned up by a periodic pass, not by status transition (see below)

**Why**: tasks may span multiple sessions. An ephemeral worktree cleaned up on exit
would lose the agent's uncommitted work on every session boundary. A persistent
worktree on a named branch survives interruption and supports true resume.

### Worktree cleanup is deferred, not immediate

Task worktrees are **not** deleted when a task transitions to a terminal state.
A human may correct an accidental terminal status transition; immediate deletion would
make that correction impossible.

Instead, a periodic cleanup pass (run alongside `clean_merged_worktrees`) removes a
task worktree only when ALL of the following are true:

1. The task is in a terminal state (`done`, `failed`, `cancelled`)
2. The task has been in that terminal state continuously for at least 72 hours
3. The worktree has no unpushed commits (`git log @{u}..HEAD` returns empty)

Condition 3 is the core safety invariant: never destroy work that has not reached the
remote. A task closed 5 minutes ago by accident is safe because a correction can be
applied before the 72-hour window expires. A task that is genuinely done will also
have its branch merged and pushed, satisfying all three conditions.

If condition 3 is not satisfiable (e.g., the remote branch was never created), the
cleanup logs a warning and skips the worktree. Manual cleanup is the escape hatch.

### Interactive `claude` runs in a detached named tmux window

Dispatch spawns:
```
tmux new-window -d -n TASK-XXXX
  -e HUB_SYSTEM_PROMPT=<kind-prompt>
  -e HUB_TASK_PROMPT=<task-context>
  -c ~/.hub/workspaces/TASK-XXXX/<project>/
  "claude --session-id <uuid>
          --dangerously-skip-permissions
          --model opus
          --append-system-prompt \"$HUB_SYSTEM_PROMPT\"
          \"$HUB_TASK_PROMPT\""
```

Key choices:
- **`new-window -d`** (detached): TUI stays visible; the TASK-XXXX window is accessible
  via the tmux status bar but does not steal focus.
- **Named window (`-n TASK-XXXX`)**: the window name is the human's primary indicator
  that a session is running and the mechanism the TUI polls for completion detection.
- **`-c <worktree>`**: the agent starts in the task's worktree, not the hub repo.
- **`--dangerously-skip-permissions`**: matches the investigation convention;
  eliminates permission-prompt stalls for file and shell operations.
- **`--session-id <uuid>`**: deterministic uuid5, computed before the dispatch
  transaction and stored in the DB atomically with the status update.

**Prompt injection** follows the pattern established in `ui/tui/src/investigations/mod.rs`:
- `--append-system-prompt "$HUB_SYSTEM_PROMPT"` — kind-specific workflow instructions,
  read from `prompts/implement-task.md`, `review-task.md`, or `debug-task.md`.
- `"$HUB_TASK_PROMPT"` — the positional `prompt` argument (first user message): task
  title, description, links, agent comment thread, and the done-when instruction
  (`hub task report TASK-XXXX --status in-review`).

Both are passed as tmux `-e` environment variables to avoid OS argument-size limits.

### Completion detection: session file polling (not `result` event or window close)

Interactive `claude` (no `-p`) **never exits when the agent finishes a turn** — the
process stays alive at an idle prompt. Neither the `result` event (assumed in 013)
nor tmux window disappearance (this decision's original fallback) reliably indicates
task completion. Both were invalidated by a lifecycle spike. The correct signals come
from `~/.claude/sessions/<pid>.json`, which Claude Code writes for every running
process.

**Revised signals:**

| Signal | Mechanism | Action |
|---|---|---|
| Agent calls `hub task report TASK-XXXX --status in-review` | CLI → DB write | Primary: immediate in-review |
| Session file `status: "idle"` >30s, no in-review in DB | TUI scans `~/.claude/sessions/` by `sessionId` | Fallback: transition to in-review |
| Session file absent, no in-review in DB | TUI scans session files | Crash recovery: transition to in-review |
| Session file `status: "busy"` + `updatedAt` stale >15 min | TUI polls `updatedAt` | Transition to `blocked` |
| Session file `status: "busy"` + fresh `updatedAt`, was blocked | TUI polls `updatedAt` | Self-heal: back to in-progress |

The session file `status` field transitions `"busy" → "idle"` when the agent finishes
a turn. `updatedAt` is epoch-ms and advances on every status change.

**Window reaping**: when the task transitions to `in-review`, the TUI schedules
`tmux kill-window -t TASK-XXXX` after a 5-minute buffer. If the task is manually moved
away from `in-review` within those 5 minutes (correction), the reap is cancelled.
Session resume is always possible via `claude --resume <session_id>`.

**`~/.claude/sessions/` is undocumented internal API.** If polling fails, tasks stay
`in-progress` until manual correction via the `s` submenu.

**Stall detection and self-heal**: if `updatedAt` has not advanced in >15 minutes while
`status` is `"busy"`, the session is likely stuck. The TUI auto-transitions to `blocked`
(High urgency). When `updatedAt` advances again, self-heal to `in-progress`. 15 minutes
avoids false positives from long Rust builds or slow clones. `blocked → in-progress`
is a fully automatic round-trip.

### Agent session logs live in `~/.hub/agent-session-logs/`

At task completion, the agent writes a markdown report and registers the path in the
task's `links` field:

```bash
# Agent writes the report
# (Path injected into the initial prompt by the dispatch workflow)
cat > ~/.hub/agent-session-logs/TASK-XXXX-<slug>.md << 'EOF'
## Summary
...
## Changes Made
...
## PRs Created
...
## Gotchas
...
## Next Steps
...
EOF

# Agent registers the path
hub task link TASK-XXXX ~/.hub/agent-session-logs/TASK-XXXX-<slug>.md
```

**Why `~/.hub/`**: colocated with all other hub system data; not inside the worktree
(which will eventually be cleaned up); accessible from any working directory.

**Why a file path in `links` alongside URLs**: the `links` field stores heterogeneous
references as a comma-separated list. The TUI already derives link type from content
characteristics (URL vs. file path: presence of `://` and `.md` extension are
sufficient discriminators). No schema change is needed. Simplicity wins over a
separate `doc_links` column.

**The logs corpus is hub's institutional memory**: the `agent-session-logs/` directory
is the primary source for mining agent pain points, recurring failures, and prompt
improvement opportunities. These files are not gitignored — they are kept locally in
`~/.hub/` and survive across repo clones and resets.

### Database home: move from macOS default to `~/.hub/hub.db`

Current: `~/Library/Application Support/hub/hub.db` (via `dirs::data_local_dir()`).

Decision: move the default to `~/.hub/hub.db`. The `HUB_DB_PATH` env var already
supports overrides; changing the default in `store/status.rs::connect()` is the only
code change required.

**Why**: `~/.hub/` is hub's system home. Having the database elsewhere breaks the
"all hub data in one place" principle and makes it harder to find, inspect, back up,
or migrate. The macOS-specific path is also inappropriate for a tool that may run on
Linux.

**Migration**: `store::connect()` checks whether the old path exists and copies it to
the new location on first run with the new default. This is a one-way, one-time
migration; no schema changes are needed.

---

## Consequences for Decision 013

- The `{"type": "result", ...}` event mentioned in 013 as the JSONL completion signal
  is superseded. Completion is now detected via session file `status: "idle"` polling
  (fallback) or agent CLI call (primary).
- `blocked` is now a meaningful auto-managed status: the TUI transitions to it on stall
  detection (`updatedAt` stale >15 min) and self-heals on activity. Prior decisions
  treated `blocked` as purely human-set.
- The JSONL mtime poll described in 013 is replaced by session file `status`/`updatedAt`
  polling across all in-progress tasks.

---

## What this decision does not resolve

- **Multi-repo workspaces**: the `~/.hub/workspaces/TASK-XXXX/<project>/` layout
  supports one repo per task. The path toward multi-repo support is adding additional
  subdirectories under `TASK-XXXX/`. The orchestration logic (which repos to clone,
  how to coordinate commits across repos) is deferred.
- **Prompt content**: `prompts/implement-task.md`, `review-task.md`, `debug-task.md`
  need to be written. The structure is decided (kind-specific system prompt +
  task-context user message); the content is authoring work, not a design decision.
- **`hub task link TASK-XXXX <path>`**: the CLI subcommand for populating `links` with
  a file path or URL. Deferred to implementation (issue #280).
