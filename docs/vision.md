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
   and a blocked PR review appear in one ranked list; their urgency is
   compared for the first time, across domain boundaries
2. **One-step task delegation** — any signal can become a task in one
   keypress; hub pre-loads context from `hub.toml` (endpoints, project
   names, linked resources) so the agent starts with zero setup
3. **TUI oversight** — the unified list shows agent sessions alongside
   signals; the detail pane streams live activity; approval or rejection
   is a single keypress, all without leaving hub

The measure of success is not "shows all the things". It's "shows the
right things, in the right order, so I can triage and delegate without
hunting — and for anything worth acting on, a task gets it moving."

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
urgency ranking (a Loki error and a stale PR compared in one list),
one-step task delegation (any signal becomes a delegated task in one
keypress), and TUI oversight (monitor agent sessions and approve results
without leaving hub).

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

## The delegation loop

Surfacing a signal is the start, not the end. The full loop:

1. **Surface** — the TUI renders a ranked list of signals from all configured
   workflows. New signal sources are added as workflows; each emits items with
   an urgency tier it defines.

2. **Create** — any signal can become a task in one keypress. The task inherits
   context from the signal (repo, description, linked issue or CI run). The human
   adds a title, optional description, and selects a kind (`implement`, `debug`,
   `general`). Tasks can also be created independently of any signal — as ideas,
   recurring maintenance work, or anything worth delegating.

3. **Delegate** — promoting the task to `ready` queues it for agent pickup. The
   system polls the ready queue and spawns a Claude Code session with the task
   context and the right skill pre-loaded from hub's config.

4. **Monitor** — the task appears in the unified list alongside other signals.
   The detail pane streams live agent activity — every file edit, command, and
   tool call — so you can watch without intervening.

5. **Approve** — when done, the agent sets the task to `in-review`. The human
   reads the summary and presses `y` (done) or `n` (back to ready with
   feedback). Nothing merges or closes without human sign-off.

This loop applies to every signal type and every kind of delegable work:
a failing CI run, a PR needing attention, a Linear issue, a production alert,
a routine maintenance task. Any signal can become a task. Any task can be
delegated.

**Skills as context providers.** Hub knows (from `hub.toml`) the Loki endpoint,
the LogQL query, the project name, the CI configuration. Skills that read this
context start with zero setup — no endpoint to look up, no query to compose.
Hub's repo is the right home for these skills: a skill added here is immediately
available for every project configured in `hub.toml`.

See [Decision 006](decisions/006-hub-as-prompt-library.md) for the skill model
and [docs/architecture/tasks.md](architecture/tasks.md) for the task lifecycle.

**Recurring tasks (future).** Tasks can be scheduled on a cadence — created and
queued automatically, without any human promotion step. This replaces manual
routine invocations (currently done via Claude Code Desktop Routines) with a
tracked, observable, approvable equivalent running inside the same loop.

## The two surfaces

**TUI (`hub-tui`)** — the human-facing surface. A Ratatui terminal dashboard
with auto-refresh, keyboard navigation, and the full delegation loop: read
signals, create tasks, monitor agent sessions, approve results. This is the
primary place to interact with hub.

**CLI (`hub`)** — the agent-facing toolkit. Agents call it during their sessions
to read their assigned task, post progress comments, and signal completion. The
system calls it on a polling loop to claim ready tasks and spawn Claude Code
sessions. Humans do not use this CLI directly.

Both surfaces share the same workflows and data layer. The UI is a render target,
not where logic lives.

See [Decision 007](decisions/007-tui-over-web-app.md) for why TUI was chosen
over a web app and what would legitimately change that decision.

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
