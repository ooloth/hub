# 018 — Meta-loop output as labeled issues

_Status: accepted; not yet implemented. Recorded ahead of the build. See
[vision.md](../vision.md) for the why._

## Context

The `mine → improve` half of the flywheel reads the corpus of agent runs
(sessions, reports, the training deltas and verdicts from
[Decision 017](017-verdict-signal-for-feedback-loop.md)) to find where an
agent's context or instructions fell short, then proposes fixes to
prompts, docs, and references.

Two questions: where do those proposals live, and what stops the loop from
flooding me with noise?

A purpose-built "proposal inbox" is a new surface to build and maintain.
But hub already aggregates an issue tracker, and the fixes target hub's own
`prompts/` and `docs/` (and the global agent harness). The tracker is
already the right home.

## Decision

### The meta-loop is itself a task kind

Mining is run by an analysis/meta task kind — the same machinery that runs
implement/debug/review tasks, pointed at the corpus instead of a feature.
The system improves itself using itself.

### Output is labeled issues, not a bespoke store

Proposals are filed as issues with a distinguishing label (e.g.
`agent-meta`) in the repo that owns the fix (hub itself for hub
prompt/doc fixes). The label _is_ the triage queue: proposals show up as
signals in hub like anything else (recursively — improvement work flows
through the same flywheel).

A separate proposal database is explicitly rejected as redundant.

### Trust-gated auto-promotion (not auto-filing)

The trust ramp governs **promotion to a dispatched task**, not whether
proposals are filed:

- **Unproven meta-loop** → proposals are filed-and-labeled only; I read the
  labeled issues and promote the good ones to `ready` tasks by hand.
- **Proven meta-loop** → high-confidence proposals auto-promote to `ready`.

This keeps the noise risk bounded while the loop earns trust, consistent
with vision.md's trust-gated scaling.

### Guard: the label is a firewall

The label must keep meta-proposals out of normal product-issue triage and
out of accidental implement-task seeding. A meta-proposal is a candidate
improvement to the system, not product work.

### Retention invariant

The corpus is the meta-loop's raw material and must be durable:

> Session JSONL, agent reports/logs, and the task row (with its typed
> origin and verdict) are **never pruned** by cleanup. Only the pushed
> worktree — a disposable working copy, safe once commits reach the remote
> — is removed. Hiding terminal tasks from the list after N days is a
> display choice, not deletion.

Today's behavior already satisfies this (worktree cleanup removes only the
working copy after 72h + no-unpushed-commits; list-hiding is display-only;
DB rows persist). This invariant makes that explicit so future cleanup
work never erases the corpus.

## Consequences

- A new analysis/meta task kind in `domain/` `TaskKind`, with its own
  prompt under `prompts/tasks/`.
- A label convention (`agent-meta` or similar) and a filing step in the
  meta-loop workflow.
- Trust-ramp configuration (per-kind auto-promotion threshold) arrives
  when the loop is proven — deferred, not built now.
- The retention invariant constrains all future task/worktree cleanup.
