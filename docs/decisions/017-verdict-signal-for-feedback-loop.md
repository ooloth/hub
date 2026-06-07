# 017 — Verdict signal for the feedback loop

_Status: accepted; not yet implemented. Recorded ahead of the build. See
[vision.md](../vision.md) for the why and [Decision 016](016-tasks-fold-back-into-signals.md)
for the typed link this depends on._

## Context

The point of the flywheel is agents whose PRs, reviews, and issues are as
good as or better than my own. That improvement is earned from one signal:
**the delta between what the agent produced and what actually shipped.**

For most work, that delta already exists on surfaces hub reads:

- **implement** → the agent's commit vs. the merged commit, plus the PR
  review comments left during review ("what did I change or catch?")
- **review** → the agent's posted review vs. what actually mattered on the PR
- **debug** → the agent's issue draft vs. the issue I edited and kept

The meta-loop (Decision 018) does not need hand-written feedback for these
— it needs to _fetch and diff_, which the typed origin from Decision 016
makes possible.

The gap is **non-PR work**. A debug task that writes an issue, or an
analysis task that produces only a report, has no PR to diff against. If I
close it `failed` or `cancelled`, the reason — the most valuable training
signal — is lost as a bare status.

## Decision

### The delta is the primary signal; fetch it, don't author it

For signal-backed, artifact-producing tasks, the training input is
computed from GitHub/Linear via the typed origin: the agent-output-vs-
shipped diff and the review thread. No new capture path is added for these.

### A minimal verdict reason for non-PR terminal transitions

When a human moves a task with no diffable artifact to `failed` or
`cancelled`, capture a minimal, machine-readable verdict reason (a short
structured line, e.g. `reason: missing-context`, with optional free text).

This is **not** a comment and **not** dialogue. The agent is done and will
never read it; it is a label on the corpus for the meta-loop. It is stored
distinctly from `task_comments` (which remain agent-authored per
[Decision 013](013-task-session-model.md)) precisely so 013's
no-bidirectional-comments rule stays intact.

### What the verdict feeds, not what it triggers

A verdict reason changes nothing about the task's lifecycle — it is pure
meta-signal. It does not re-dispatch (013 still holds: retry means a new
task with improved inputs), and it is never surfaced to a running agent.

## Consequences

- A small `verdict` field on the task (reason enum + optional note),
  written only at human-driven `failed`/`cancelled` of non-PR tasks.
- The TUI fail/cancel path for such tasks prompts for the reason; for
  PR-backed tasks it does not (the delta is on GitHub).
- The meta-loop ([Decision 018](018-meta-loop-output-as-labeled-issues.md))
  gets a uniform input: diffs for PR work, verdict reasons for the rest.
- Decision 013 is unaffected: verdicts are corpus annotations, not the
  human half of an agent conversation.
