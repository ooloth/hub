//! Claude Code session state detection for the task dispatch pipeline.
//!
//! This module is the **only place** that reads `~/.claude/sessions/<pid>.json`.
//! If that file format changes or a different agent runner is used, only this
//! module needs updating — the rest of the dispatch pipeline is unaffected.
//!
//! # Signals used
//!
//! Claude Code writes a JSON state file for every running process:
//!
//! ```json
//! {
//!   "pid":       42899,
//!   "sessionId": "<uuid5 injected at dispatch>",
//!   "cwd":       "<worktree path>",
//!   "status":    "busy" | "idle",
//!   "updatedAt": <epoch-ms, changes on every status transition>
//! }
//! ```
//!
//! Hub infers agent progress from `status` and `updatedAt`:
//!
//! | Session file state | Condition | Task transition |
//! |---|---|---|
//! | present, `status="busy"` | `updatedAt` advancing | stay `in-progress` |
//! | present, `status="busy"` | `updatedAt` stale >15 min | → `blocked` |
//! | present, `status="idle"` | >30s, no `in-review` in DB | → `in-review` (fallback) |
//! | absent | no `in-review` in DB | → `in-review` (crash recovery) |
//! | was `blocked`, `status="busy"` | `updatedAt` advancing | → `in-progress` (self-heal) |
//!
//! See `docs/architecture/task-dispatch.md` "Session file signal reference" for
//! the full table and risk notes.
//!
//! # Finding the session file
//!
//! Hub injects `--session-id <uuid5>` at dispatch and stores the value in
//! `tasks.session_id`. To locate the running process's state file, scan
//! `~/.claude/sessions/*.json` for a record whose `sessionId` field matches.
//! The PID is unknown at dispatch time; the UUID is the stable identifier.
//!
//! # Risk
//!
//! `~/.claude/sessions/` is an **undocumented internal API**. Anthropic could
//! change or remove it without notice. If polling fails silently, tasks stay
//! `in-progress` until manual correction via the `s` submenu. The primary
//! signal (`hub task report` CLI call) is independent and unaffected.
//!
//! # Implementation
//!
//! Not yet built. Tracked in issue #281 (S3).

// Implementation goes here (S3).
