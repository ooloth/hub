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

The design principle: **workflows classify, hub aggregates.**

Each workflow emits items with an urgency tier it defines. A
production error is always higher urgency than a PR waiting for review
— that's domain knowledge the workflow holds, not something a
central system can infer. Hub sorts by `(urgency, age)` and renders.

Urgency tiers: `Critical → High → Medium → Low`

The rule-based approach comes first. AI-assisted scoring is a natural
later layer when rules feel limiting — but starting with rules forces
clarity about what "urgent" actually means per workflow.

## The "everything hub" failure mode

Tools that show everything become graveyards. You stop checking them
because they're always full. Hub avoids this by being opinionated:
items that don't need action today shouldn't appear. Each workflow
is responsible for filtering its own noise before emitting items.

## Workflows

Each workflow lives in `clients/<name>/` and `workflows/`. Adding
one means adding files — no central registration.

Current:

| Workflow | What it tracks |
|---|---|
| GitHub PRs | PRs awaiting my review |

Planned:

| Workflow | What it tracks |
|---|---|
| Production errors | Errors/exceptions from logs (Loki, Axiom) |
| Issues | Linear/Jira tickets assigned to me |
| CI | Failing runs on watched repos |
| Home server | Health and availability (private workflow) |

Future candidates: dependency alerts, Notion tasks, calendar conflicts.

## Investigation

Surfacing a signal is not the same as understanding it. Hub goes one
level deeper: when a signal warrants it, an investigation skill can
diagnose what's happening.

Investigation skills are Claude Code skills that live in hub's
`.claude/skills/` directory. They are multi-turn conversations — Claude
uses CLI tools (`logcli`, `gh`, etc.) to query data iteratively,
forming hypotheses and validating them, until it can produce a
diagnosis. This is distinct from the `agents/` crate, which handles
single-call, unattended background automation.

Hub's role in this layer is **context provider**. Hub knows (from
`hub.toml`) the Loki endpoint for a project's production environment,
the LogQL query that selects the right app, the project name. A skill
that reads this context can be invoked with zero setup — no endpoint
to look up, no query to compose from scratch. The investigation starts
immediately.

```
hub status                # "prod: 12 errors in last hour (3× baseline)"
claude /loki-investigate  # iterates until diagnosed; hub.toml provides context
```

Hub's repo is also the right home for these skills — not each project's
repo. A skill added to hub is immediately available for every project
configured in `hub.toml`, without copy-pasting it across repos.

See [Decision 006](decisions/006-hub-as-skill-library.md) for the full model.

## UI evolution

1. **CLI** — `hub status` prints a ranked list to the terminal. Fast,
   scriptable, works from anywhere. Current state.
2. **TUI** — a Ratatui terminal dashboard with panels per workflow,
   auto-refresh, and keyboard navigation. The "command center"
   aesthetic. Planned next. The TUI is not just a display: it is a
   place to zoom in. You see everything you're responsible for at a
   glance, then press a key on a signal to launch the investigation
   skill for it — with context pre-loaded from hub's config.

Both entry points share the same workflows and data layer. The UI is a
render target, not where logic lives.

## What this is not

- A web app (no server, no browser, no HTML)
- A team tool (single-user, single-device, no sharing)
- A notification system (pull, not push — you open hub when you want
  to triage, it doesn't interrupt you)
- A passive display — hub is a place to act, not just observe; signals
  link to the investigation and action tools that resolve them
