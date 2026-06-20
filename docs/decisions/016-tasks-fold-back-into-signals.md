# 016 — Tasks fold back into signals

> **⚠ Superseded by [Decision 019](019-drop-task-model-filesystem-sessions.md) (accepted).**
> Fold-back is removed entirely — with no task there is no second identity to
> sync. A signal's own lifecycle encodes done-ness (the row disappears when it
> resolves). Recorded here for rationale and history.

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
as a string in `links`. Final shape, decided at build ([issue 290](https://github.com/ooloth/hub/issues/290)):

```
enum TaskOrigin {
    Pr    { repo: RepoSlug, number: u64 },
    Issue { system: IssueSystem, repo: Option<RepoSlug>, id: String },
    Ci    { repo: RepoSlug, workflow: String, job: Option<String>, step: Option<String>, url: String },
    Alert { source: AlertSource, key: String, label: String },
    Idea,                                     // no signal (blank task)
}

enum IssueSystem { GitHub, Linear }           // extensible: Jira, Monday, Trekker
enum AlertSource { Loki, Gcp, Media }         // extensible: any future scanner
```

This is a newtype-style domain concept (per CLAUDE.md), distinct from the
freeform `links` field, which stays for arbitrary supplementary references.
`repo` becomes derivable from the origin for the variants that carry it.

Three shape decisions worth recording, because the illustrative sketch above
them was wrong on each:

- **One `Issue` variant for every ticket system**, tagged by `system`, rather
  than a variant per system. Lets the TUI filter "all tickets regardless of
  system" as a single match. The `id` is the system-native key (`#42` for
  GitHub, `ENG-123` for Linear) — a join key re-parsed on refetch, not typed
  per system. `repo` is `None` for trackers without one (Linear). The
  illustrative `run_id`/`fingerprint` fields don't exist on the source signals.
  `Ci` keys on `(repo, workflow, job, step)` — all already on `CiFailure` and
  invariant for the same failing check — keeping `url` only for navigation (it
  changes every run); `Alert` keys on a derived grouping string.
- **One `Alert` variant for every scan source** (Loki, GCP, media), not one per
  source. PR/Issue/CI earn dedicated variants because they have an external
  tracker with a refetchable join key; Loki/GCP/media are the same class —
  internally-detected operational problems with no external ticket identity, so
  they share a variant. `key` is a stable grouping/fingerprint each source
  derives its own way; `label` is the display string.
- **`TaskOrigin` is `private`-feature-independent.** It lives in `domain` (which
  has no `private` feature and must never gain one), and no variant — including
  `AlertSource::Media` — is `#[cfg(feature = "private")]`-gated. Persisted task
  data outlives any single build config: a non-`private` build (a device with no
  media signals) must deserialize and render every origin a `private` build
  wrote, including `Alert { source: Media }`. Only the *construction* of a
  media-sourced alert at seed time is `private`-gated; the type is total on every
  build. A gated variant would reproduce the existing `StatusItem` hazard, where
  a non-`private` build cannot deserialize a payload a `private` build serialized.

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

**One active task per signal.** The badge assumes at most one non-terminal
task exists for a given signal. Two active agents working the same signal
is not a meaningful use case — it's either a mistake or a prior task that
wasn't closed. This invariant is enforced at dispatch time: before creating
a task, the dispatch workflow checks whether a non-terminal task already
exists for the same origin. If one does, dispatch is blocked and the human
is prompted to close the existing task first. Enforcing it at dispatch
(rather than at the DB or render layer) keeps the render logic simple:
a signal row always has zero or one task to badge, never a set to choose
from.

**Detail-pane access.** Suppressing the task row is only safe if the
session transcript and task metadata remain reachable. Selecting a badged
signal row and pressing a second key opens the task session detail view.
This toggle is in scope for the badge+dedup build ([issue 294](https://github.com/ooloth/hub/issues/294)),
not a follow-on.

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
