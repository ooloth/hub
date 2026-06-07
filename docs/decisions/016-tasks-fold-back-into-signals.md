# 016 — Tasks fold back into signals

_Status: accepted; not yet implemented. Recorded ahead of the build so the
work lands the right shape. See [vision.md](../vision.md) for the why._

## Context

A task can carry an agent from a signal (a PR, an issue, a CI failure, a
Loki alert) to a delivered artifact. But hub currently has no structured
knowledge of that relationship. A task records its origin only in a
freeform `links: Vec<String>` (stored as a CSV `links TEXT` column) and an
optional `repo`. A task created from PR #42 is indistinguishable from a
task someone typed by hand that happens to mention the same URL.

Two costs follow:

- **The double touchpoint.** When an agent finishes an implement task and
  opens a PR, the human reviews and merges the PR on GitHub — and then has
  to visit the task and hand-move it to `done`. One mental event ("this
  work is accepted"), two manual actions in two places. This is the
  friction that makes tasks feel like bookkeeping.
- **Mining is impossible.** The feedback loop (Decision 017) needs to
  compare what the agent produced against what shipped. It can't, because
  hub doesn't know which artifact a task produced.

## Decision

### The touchpoint principle

A task must never require a human action that duplicates an action on its
underlying signal. **A task's terminal transition is a _consequence_ of the
signal's terminal transition, not a separate decision.** The human acts on
the PR/issue/alert; the task follows.

The only times a human touches a task's status directly are a task with no
underlying signal (a pure idea), or a `blocked`/`failed` task that needs
redirection. The status submenu remains as that escape hatch.

### Typed origin

A task records the signal it came from as a typed value in `domain/`, not
as a string in `links`. Shape (illustrative, final form decided at build):

```
enum TaskOrigin {
    Pr    { repo: RepoSlug, number: u64 },
    Issue { repo: RepoSlug, number: u64 },   // or Linear id
    Ci    { repo: RepoSlug, run_id: u64 },
    Alert { fingerprint: String },
    Idea,                                     // no signal
}
```

This is a newtype-style domain concept (per CLAUDE.md), distinct from the
freeform `links` field, which stays for arbitrary supplementary references.
`repo` becomes derivable from the origin for signal-backed tasks.

### Fold-back inference

During the fetch loop hub already runs, for any non-terminal task with a
signal-backed origin, infer the terminal transition from the signal:

- linked PR merged → task `done`
- linked PR closed unmerged → task `failed`
- linked issue/alert resolved → task `done` (for tasks whose product is
  the issue/alert resolution, e.g. debug)

Inference is one-directional (signal terminal → task terminal) and applies
only to non-terminal tasks, so a human correction is never overwritten.

### Badge and dedup

A signal row that has an attached task shows it inline (`⊛ TASK-42 ·
in-progress`) rather than as a second, competing row. The same unit of
work appears once, enriched — never as both a PR row and a task row the
human must mentally correlate.

## Consequences

- Schema migration: add origin columns to the `tasks` table; backfill is
  trivial (existing tasks become `Idea` or keep their `repo`).
- The fetch workflow gains a fold-back step that reads linked-signal state
  and transitions matching tasks. No new polling system — it rides the
  existing refresh.
- The TUI unified list gains badge + dedup logic on signal rows.
- This is the keystone for [Decision 017](017-verdict-signal-for-feedback-loop.md):
  the typed origin is the join key that makes the training delta
  computable.
- [Decision 012](012-task-model.md)'s `links` field is no longer the
  origin record; it reverts to its plain meaning (supplementary links).
