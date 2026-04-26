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
This loop applies to every domain. The integrations are just different
sources feeding the same loop.

The measure of success is not "shows all the things". It's "shows the
right things, in the right order, so I can triage and act without
hunting."

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
are dashboards, not prioritization. Hub aims to answer: *why does this
need my attention today?*

The design principle: **integrations classify, hub aggregates.**

Each integration emits items with an urgency tier it defines. A
production error is always higher urgency than a PR waiting for review
— that's domain knowledge the integration holds, not something a
central system can infer. Hub sorts by `(urgency, age)` and renders.

Urgency tiers: `Critical → High → Medium → Low`

The rule-based approach comes first. AI-assisted scoring is a natural
later layer when rules feel limiting — but starting with rules forces
clarity about what "urgent" actually means per integration.

## The "everything hub" failure mode

Tools that show everything become graveyards. You stop checking them
because they're always full. Hub avoids this by being opinionated:
items that don't need action today shouldn't appear. Each integration
is responsible for filtering its own noise before emitting items.

## Integrations

Each integration lives in `clients/<name>/` and `workflows/`. Adding
one means adding files — no central registration.

Planned and current:

| Integration | What it tracks |
|---|---|
| GitHub PRs | PRs awaiting my review |
| Production errors | Errors/exceptions from logs (Loki, Axiom) |

Future candidates: Linear/Jira issues assigned to me, failing CI runs,
dependency alerts, Notion tasks, calendar conflicts, home server health
(via private integrations).

## UI evolution

1. **CLI** — `hub status` prints a ranked list to the terminal. Fast,
   scriptable, works from anywhere. Current state.
2. **TUI** — a Ratatui terminal dashboard with panels per integration,
   auto-refresh, and keyboard navigation. The "command center"
   aesthetic. Planned next.

Both entry points share the same workflows and data layer. The UI is a
render target, not where logic lives.

## What this is not

- A web app (no server, no browser, no HTML)
- A team tool (single-user, single-device, no sharing)
- A notification system (pull, not push — you open hub when you want
  to triage, it doesn't interrupt you)
- A replacement for the source tools (it surfaces items; you act on
  them in GitHub, your log dashboard, etc.)
