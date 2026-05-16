# Vision

## The problem

Too many places to check. GitHub notifications, production errors, log
dashboards, issue trackers, PR queues — each one is a separate context
switch, and together they make it easy to miss what actually matters.
The question "what do I need to act on today?" has no single answer.

## What hub is

A personal command center to observe and act across any terrain I'm
responsible for: software, infrastructure, home systems, whatever.

The core loop: observe signals → understand what they mean → decide
what to do → act (or delegate to an agent) → learn from outcomes.
This loop applies to every domain. The workflows are just different
sources feeding the same loop.

Hub is the aggregator — it pulls every project in via `hub.toml` rather
than being installed into each project. This inversion matters: urgency
can only be compared across domains from a central vantage point, and
execution workflows can be defined once for all opted-in repos without
copy-pasting orchestration scripts across the codebase.

Other tools already surface individual signals. Hub adds three things
they don't:

1. **Cross-domain urgency ranking** — a Loki error, a failing CI run,
   and a blocked home server download appear in one ranked list; their
   urgency is compared for the first time, across domain boundaries
2. **Pre-loaded investigation context** — hub.toml holds the Loki
   endpoint, the LogQL query, the project name; a keypress launches the
   right Claude Code skill with zero setup; the investigation starts
   immediately
3. **Keypress execution** — for issues labelled `status:ready-for-agent`,
   a keypress in the TUI launches an agent in a git worktree; hub handles
   setup so the focus is on reviewing the result, not orchestrating the run

The measure of success is not "shows all the things". It's "shows the
right things, in the right order, so I can triage and act without
hunting — and for the ready stuff, a keypress gets it moving."

Agents are a first-class tool. Where rules are sufficient, use rules.
Where judgment or scale makes agents more appropriate, use agents. The
output format (dashboard item, report, alert, GitHub issue, automated
fix) is chosen to match the signal — not fixed to one mode.

## Context-awareness

Each device has its own SQLite database and its own config. Work laptop
shows work software; personal laptop shows personal software. There is
no cloud sync and no shared state between devices. This is intentional
— work and personal contexts have different tools, different urgency
thresholds, and different audiences.

## Prioritization

The hardest problem is signal vs noise. Raw counts ("5 PRs, 3 errors")
are dashboards, not prioritization. Hub aims to answer: _why does this
need my attention today?_

The design principle: **workflows classify, hub aggregates.**

Each workflow emits items with an urgency tier it defines. A
production error is always higher urgency than a PR waiting for review
— that's domain knowledge the workflow holds, not something a
central system can infer. Hub sorts by `(urgency, age)` and renders.

Urgency tiers: `Critical → High → Medium → Low`

The rule-based approach comes first. AI-assisted scoring is a natural
later layer when rules feel limiting — but starting with rules forces
clarity about what "urgent" actually means per workflow.

## The two failure modes

**Failure mode 1: shows everything.** Tools that show everything become
graveyards. You stop checking because they're always full. Hub avoids
this by being opinionated: items that don't need action today shouldn't
appear. Each workflow is responsible for filtering its own noise before
emitting items.

**Failure mode 2: only mirrors what you'd see elsewhere.** A tool that
wraps GitHub, Linear, and Grafana without adding a triage or agency
layer is just slower than each individual source. Grafana can monitor
production errors. Scripts can check disk usage. Browser tabs can
aggregate dashboards. The monitoring itself is not the invention.

Hub avoids this by adding the things source tools don't: cross-domain
urgency ranking (a Loki error and a blocked download compared in one
list), pre-loaded investigation context (one keypress to diagnose, not
five minutes of setup), and keypress execution (a ready issue goes to an
agent without leaving hub).

The bar for a new workflow: does it contribute to cross-domain triage,
speed up investigation, or enable automated proposals? A workflow that
only mirrors one source without adding any of these layers doesn't pull
its weight.

## Workflows

Each workflow lives in `clients/<name>/` and `workflows/`. Adding
one means adding files — no central registration.

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

Future candidates: Notion tasks, calendar conflicts.

Private workflows live in hub-private and are compiled in under
`#[cfg(feature = "private")]`. They follow the same architecture as
public workflows but reference infrastructure that isn't in the public repo.

## Investigation

Surfacing a signal is not the same as understanding it. Hub goes one
level deeper: when a signal warrants it, an investigation skill can
diagnose what's happening.

Investigation skills are Claude Code skills that live in hub's
`.claude/skills/` directory. They are multi-turn conversations — Claude
uses CLI tools (`logcli`, `gh`, etc.) to query data iteratively,
forming hypotheses and validating them, until it can produce a
diagnosis.

Hub's role in this layer is **context provider**. Hub knows (from
`hub.toml`) the Loki endpoint for a project's production environment,
the LogQL query that selects the right app, the project name. A skill
that reads this context can be invoked with zero setup — no endpoint
to look up, no query to compose from scratch. The investigation starts
immediately.

```
hub status                      # "github ci (1)  ooloth/hub  CI  failure  0h"
claude /github-ci-investigate   # fetches failed step logs and surfaces root cause

hub status                      # "prod: 12 errors in last hour (3× baseline)"
claude /loki-investigate        # iterates until diagnosed; hub.toml provides context (planned)
```

Hub's repo is also the right home for these skills — not each project's
repo. A skill added to hub is immediately available for every project
configured in `hub.toml`, without copy-pasting it across repos.

See [Decision 006](decisions/006-hub-as-prompt-library.md) for the full model.

## The three tiers

Hub responds to signals at three levels of automation. Each workflow starts at Tier 1 and
graduates to higher tiers as the signal patterns become well understood.

**Tier 1 — Surface.** `hub status` emits a ranked list. You see the signal; you decide
whether to act. This is always the starting point for a new workflow.

**Tier 2 — Investigate.** A keypress on a ranked item launches the right Claude Code skill
with `hub.toml` context pre-loaded. Claude iterates — querying logs, fetching CI output,
checking API state — until it produces a diagnosis. You're in the loop; Claude is the
analyst. The investigation is multi-turn and human-supervised.

**Tier 3 — Execute.** For issues labelled `status:ready-for-agent`, a keypress in the TUI
launches an agent in a git worktree. Hub handles setup (worktree, credentials, private wiring)
so the agent starts immediately with the right context. You watch it work in a tmux split;
approval is always required before anything merges.

Not every signal reaches Tier 3 — some require human judgment every time. The graduation path
is: surface the signal (Tier 1), then investigate until the diagnosis steps are repeatable
(Tier 2), then — if the fix itself is mechanical enough to delegate — add an execution action
(Tier 3).

## UI evolution

1. **CLI** — `hub status` prints a ranked list to the terminal. Fast,
   scriptable, works from anywhere.
2. **TUI** — a Ratatui terminal dashboard with panels per workflow,
   auto-refresh, and keyboard navigation. The TUI is not just a display:
   it is a place to zoom in. You see everything you're responsible for at
   a glance, then press a key on a signal to launch the investigation
   skill for it — with context pre-loaded from hub's config.

Both entry points share the same workflows and data layer. The UI is a
render target, not where logic lives.

The TUI is not a chat interface — Claude Code is. Hub's job is to surface
signals and hand off to the right investigation skill with context
pre-loaded. A keybinding that opens `claude /loki-investigate` in a new
tmux pane, with hub.toml already providing the endpoint, query, and project
name, is the complete agent integration story. Hub is the launcher; Claude
Code is the investigator.

The TUI is the place to dispatch, watch, and review execution work
without leaving hub. A keypress on a ready issue sends the agent to work
in a tmux split; a keypress on a completed job opens the PR for review.

See [Decision 007](decisions/007-tui-over-web-app.md) for why TUI was
chosen over a web app and what would legitimately change that decision.

## What this is not

- A web app — no server, no browser, no HTML. The terminal is the right
  home: hub lives where the developer already works, requires no build step
  or deployment, and stays local-only. When a feature seems to call for
  browser UI, that's usually a sign the feature isn't core. The constraints
  are a feature, not a limitation. See [Decision 007](decisions/007-tui-over-web-app.md).
- A team tool (single-user, single-device, no sharing)
- A notification system (pull, not push — you open hub when you want
  to triage, it doesn't interrupt you)
- A passive display — hub is a place to act, not just observe; signals
  link to the investigation and action tools that resolve them
