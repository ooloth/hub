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
1. DEFINE   a signal becomes an agent session (launched with `i`)
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

### Signals are the spine; sessions are work on them

The **signal** is the unit of work — a PR to review, a CI failure to fix, an
alert to investigate. Hub does not wrap a signal in a second identity (a
TASK-XXXX); an agent **session** is simply work done *on* a signal, launched
with `i`. A session makes that work **visible** (the signal row badges when
sessions exist for it), **resumable** (its tmux window stays open — attach to
course-correct), **reviewable** (its `report.md` and the artifact it produced
lead me to the agent's reasoning), and **mineable** (its `prompt.md`,
`report.md`, and transcript are raw material for improving future agents).

This is *not* a parallel project-management layer — no kanban board, no second
inbox, no bookkeeping. The session record is just the join point between a
signal, an agent run, and the resulting artifact. See
[Decision 019](decisions/019-drop-task-model-filesystem-sessions.md).

### No second touchpoint, by construction

The touchpoint principle: **work on a signal must never require an action that
duplicates an action on the signal itself.** Because hub no longer creates a
second identity for the work, there is nothing to keep in sync and nothing to
"fold back."

- Work on a PR (review or implement) resolves when I review and merge the PR
  exactly as I would without hub. The signal row disappears on its next
  refresh; that _is_ completion.
- A **debug** session on an alert resolves when the alert clears — or, when it
  produces only findings, its `report.md` is itself the artifact.

The signal's own lifecycle already encodes done-ness, so there is no task
status to transition and no fold-back machinery. That is what makes this feel
like _one_ workflow, not two. See
[Decision 019](decisions/019-drop-task-model-filesystem-sessions.md).

### The training signal

"Agents better than me" is earned from one specific signal: **the delta
between what the agent produced and what actually shipped.**

- implement → the agent's commit vs. the merged commit, plus my PR review
  comments ("what did I change or catch?")
- review → the agent's review vs. what actually mattered on that PR
- debug → the agent's issue draft vs. the issue I edited and kept

Almost all of that delta already lives on GitHub and Linear. The meta-loop
does not need me to hand-write feedback — it needs to _fetch and diff_. What
makes this computable is knowing which artifact a session produced; rather than
recording a typed link during the run, hub reconstructs it post-hoc from the
session's transcript (the `gh pr create` / `git push` tool calls) and from
GitHub state. See
[Decision 019](decisions/019-drop-task-model-filesystem-sessions.md).

### Trust-gated scaling

Running ten sessions at once is a _trust outcome_, not a feature to switch on.
Today concurrency is self-throttled — I start as many `i` sessions as I can
review. Machine-initiated dispatch (future scheduled routines) and auto-merge
follow earned trust, never the reverse: enabling them before the flywheel has
turned enough just ships bad changes faster.

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
auto-refresh and keyboard navigation: read signals, watch investigation
sessions, review results. This is the primary place to interact with hub.

**CLI (`hub`)** — hub's command-line surface. The task model (task-claiming
loop, `hub task *` protocol) was removed by
[Decision 019](decisions/019-drop-task-model-filesystem-sessions.md). The CLI
is a stub; agent-facing session-toolkit subcommands will be added here as the
filesystem session model is built out. Humans do not use it for triage — that
is the TUI.

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

- **A kanban / todo board** — hub isn't a todo app; work lives on its signal,
  not in swim lanes.
- **A bidirectional comment thread** — a session's `report.md` is an
  agent→human account, not a chat. Dialogue happens by attaching to the
  session's tmux window, not by typing back. See
  [Decision 019](decisions/019-drop-task-model-filesystem-sessions.md).
- **A quality gate** — clearing a signal means "I acknowledged and closed
  this," not "I certify this is correct." GitHub PR review and merge is the
  real quality signal.
- **A manual signal↔artifact correlation UI** — I never hand-match work to the
  PR it produced; the link is reconstructed from the session transcript and
  GitHub state.
- **Auto-merge — for now** — merging stays on GitHub. Autonomy ramps only
  as a workflow-kind earns trust (see Trust-gated scaling).

See [Decision 019](decisions/019-drop-task-model-filesystem-sessions.md) for the
filesystem session model that replaces the task model.
