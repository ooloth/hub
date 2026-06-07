# Vision

> **Read this first.** This is hub's north star — the *why*. Everything else
> points back here. `architecture/` describes what is *built today*; this
> document and the `decisions/` ADRs describe where we are *going*.

## The problem

I have two bottlenecks, not one.

**Triage.** Too many places to check — GitHub notifications, production
errors, log dashboards, issue trackers, PR queues. Each is a separate
context switch, and together they make it easy to miss what actually
matters. "What do I need to act on today?" has no single answer.

**Throughput.** Even once I know what to do, the work goes through agents
I have to steer synchronously — kick off a session, wait for a turn,
context-switch to something else, come back, redirect, repeat. That
babysitting is draining and it caps the total terrain I can cover before
I'm out of focus for the day. The ceiling isn't my typing speed; it's how
much I can personally supervise.

Hub attacks both: it ranks signals across domains so triage is one glance,
and it builds agents I can *trust* to make choices as good as or better
than my own — so the constraint moves from _my attention_ to _my review
throughput_, which is a far higher ceiling.

## What hub is

A personal command center to observe and act across any terrain I'm
responsible for: software, infrastructure, home systems, whatever.

Hub is the aggregator — it pulls every project in via `hub.toml` rather
than being installed into each project. This inversion matters: urgency
can only be compared across domains from a central vantage point, and
execution workflows can be defined once for all opted-in repos without
copy-pasting orchestration across codebases.

But aggregation is table stakes. The deeper purpose is the **flywheel**
below: hub turns scattered signals into delegated agent work, makes that
work visible and reviewable, and mines every run to make the next agent
sharper. Aggregation is the entry point; compounding agent quality is the
point.

## The flywheel

This is the core loop and the reason hub exists. Each turn makes the next
turn cheaper, because the agents get better at making the choices I'd make.

```
1. DEFINE   a signal or idea becomes a dispatched task
2. RUN      an agent works async; I watch live, resume if needed
3. REVIEW   I judge the work PRODUCT on its native surface (the PR/issue),
            fast — because the agent supplied proof it's correct
4. MINE     a meta-agent reads the corpus + the downstream truth to find
            where the agent's context or instructions fell short
5. IMPROVE  findings become fixes to prompts/docs/references — raising the
            floor for ALL future agents
            ↓ loop. As trust grows, run more in parallel, supervise less.
```

The dream this enables: kick off five to ten well-defined pieces of work
in the morning and have them land as shipped improvements by end of day —
not because I typed faster, but because I trusted more.

### Tasks are the spine

A Task is the unit that makes one piece of delegated agent work
**visible** (it shows in the list, streaming live activity),
**resumable** (reopening the session to course-correct — the `o` key is
planned, [issue 289](https://github.com/ooloth/hub/issues/289)),
**reviewable** (its report and links lead me straight to the artifact and
the agent's reasoning), and **mineable** (its session, report, and verdict
are raw material for improving future agents).

Tasks are *not* a parallel project-management layer. They are not a kanban
board, not a second inbox, not a place I do bookkeeping. They are the
join point between a signal, an agent run, and the resulting artifact.

### Tasks fold back into signals

The touchpoint principle: **a task must never require an action that
duplicates an action on its underlying signal.** A task's terminal
transition is a _consequence_ of the signal's, not a separate decision.

- An **implement** task folds into the PR it produces. I review and merge
  the PR exactly as I would without hub; merging closes the task, closing
  it unmerged fails it. Zero extra touchpoints on the happy path.
- A **review** task folds into the PR it reviewed. The agent's posted
  review is _input_ to my decision, not a duplicate of it.
- A **debug** task that produces only findings is the one case where the
  task itself is the artifact — or it folds into the alert it explains.

So the only time I touch a task's status directly is a pure idea with no
signal, or a `blocked`/`failed` task that needs redirection. Everything
else dissolves on its own. That is what makes tasks feel like _one_
workflow, not two.

### The training signal

"Agents better than me" is earned from one specific signal: **the delta
between what the agent produced and what actually shipped.**

- implement → the agent's commit vs. the merged commit, plus my PR review
  comments ("what did I change or catch?")
- review → the agent's review vs. what actually mattered on that PR
- debug → the agent's issue draft vs. the issue I edited and kept

Almost all of that delta already lives on GitHub and Linear. The meta-loop
does not need me to hand-write feedback — it needs to _fetch and diff_.
The only thing that makes this computable is hub knowing which artifact a
task produced, which is why a typed task↔signal link is the keystone the
rest of the flywheel depends on (see
[Decision 016](decisions/016-tasks-fold-back-into-signals.md)).

### Trust-gated scaling

Running ten tasks at once is a _trust outcome_, not a feature to switch on.
Concurrency and autonomy follow earned trust, never the reverse. Raising
the dispatch cap or enabling auto-merge before the flywheel has turned
enough just ships bad changes faster.

So the build order optimizes the loop first — define → review → mine →
improve — and lets capacity and autonomy ramp _per workflow-kind_ as each
proves itself. Hub accelerates the trust-building; it does not grant trust.

## Cross-domain triage

Other tools surface individual signals. Hub's triage layer adds what they
don't: a Loki error, a failing CI run, and a blocked PR review appear in
one ranked list, their urgency compared for the first time across domain
boundaries.

The hardest problem is signal vs. noise. Raw counts ("5 PRs, 3 errors")
are dashboards, not prioritization. Hub aims to answer: _why does this
need my attention today?_

The design principle: **workflows classify, hub aggregates.** Each
workflow emits items with an urgency tier it defines — a production error
is always higher urgency than a PR waiting for review, and that's domain
knowledge the workflow holds, not something a central system can infer.
Hub sorts by `(urgency, age)` and renders.

Urgency tiers: `Critical → High → Medium → Low`. Rules come first;
AI-assisted scoring is a natural later layer when rules feel limiting.

### The two failure modes

**Shows everything.** Tools that show everything become graveyards — you
stop checking because they're always full. Hub stays opinionated: items
that don't need action today shouldn't appear. Each workflow filters its
own noise before emitting.

**Only mirrors what you'd see elsewhere.** A tool that wraps GitHub,
Linear, and Grafana without adding a triage or agency layer is just slower
than each source. The monitoring is not the invention — the cross-domain
ranking, the delegation, and the trust-building flywheel are.

## Context-awareness

Each device has its own SQLite database and its own config. Work laptop
shows work software; personal laptop shows personal software. No cloud
sync, no shared state between devices — work and personal contexts have
different tools, urgency thresholds, and audiences.

## Workflows

Each workflow lives in `clients/<name>/` and `workflows/`. Adding one
means adding files — no central registration.

Current:

| Workflow      | What it tracks                                      |
| ------------- | --------------------------------------------------- |
| GitHub PRs    | PRs awaiting my review                              |
| GitHub issues | Open issues assigned to me or open in watched repos |
| GitHub CI     | Failing workflow runs on watched repos              |
| Linear        | Incomplete issues assigned to me                    |

Planned (public repo — some may already exist in hub-private):

| Workflow          | What it tracks                                      |
| ----------------- | --------------------------------------------------- |
| Production errors | Error-level log entries (Loki) — first novel signal |
| Dependabot        | Security alerts on watched repos                    |
| Home server: disk | Drive usage on media drives                         |

Private workflows live in hub-private and compile in under
`#[cfg(feature = "private")]`. They follow the same architecture as public
workflows but reference infrastructure that isn't in the public repo.

**Skills as context providers.** Hub knows (from `hub.toml`) the Loki
endpoint, the LogQL query, the project name, the CI configuration. Skills
that read this context start with zero setup. Hub's repo is the right home
for them: a skill added here is immediately available for every configured
project. These prompts and skills are the surface the meta-loop tunes —
sharpening them is _how_ agent quality compounds. See
[Decision 006](decisions/006-hub-as-prompt-library.md).

## The two surfaces

**TUI (`hub-tui`)** — the human-facing surface. A Ratatui dashboard with
auto-refresh and keyboard navigation: read signals, create tasks, watch
agent sessions stream live, review results. This is the primary place to
interact with hub.

**CLI (`hub`)** — the agent-facing toolkit. Agents call it during their
sessions to read their assigned task, write a captain's-log comment, and
signal completion. The system calls it on a polling loop to claim ready
tasks and spawn sessions. Humans do not use this CLI directly.

Both surfaces share the same workflows and data layer. The UI is a render
target, not where logic lives. See
[Decision 007](decisions/007-tui-over-web-app.md).

## What hub is not

- **A web app** — no server, no browser, no HTML. The terminal is where I
  already work; the constraints are a feature. See
  [Decision 007](decisions/007-tui-over-web-app.md).
- **A team tool** — single-user, single-device, no sharing.
- **A notification system** — pull, not push. I open hub when I want to
  triage; it doesn't interrupt me.
- **A passive display** — hub is a place to act, not just observe.

And, specifically, what the flywheel does **not** build:

- **A kanban / todo board** — hub isn't a todo app; tasks fold into
  signals, they don't live in swim lanes.
- **A bidirectional comment thread** — comments are an agent→human
  captain's log. Dialogue happens by resuming the session (planned: `o`), not by
  typing back. See [Decision 013](decisions/013-task-session-model.md).
- **A quality gate** — `done` means "I acknowledged and closed this," not
  "I certify this is correct." GitHub PR review and merge is the real
  quality signal.
- **A manual task↔signal correlation UI** — the link is typed and
  automatic; I never hand-match a task to the PR it produced.
- **Auto-merge — for now** — merging stays on GitHub. Autonomy ramps only
  as a workflow-kind earns trust (see Trust-gated scaling).

See [docs/architecture/tasks.md](architecture/tasks.md) for the task model
as built, and [docs/architecture/task-dispatch.md](architecture/task-dispatch.md)
for how dispatch runs today.
