---
opened: 2026-10-17
status: open
resolves_into: decision
---

# Should hub show a system overview for each project?

## Why it matters

Hub shows only what needs attention today. The vision names "shows everything" as a failure mode
and says items that don't need action today shouldn't appear. A system overview (every deployed
resource, healthy or not, recent runtime logs, and standard actions per project) contradicts that
as written. Adding one without amending the vision leaves hub's scope unclear. Adding it in the
wrong place lets healthy-state rows compete with the triage list for attention, which is how the
"shows everything" failure starts.

It also tests whether terminal cockpits and agent launchers are worth building for anyone besides
the maintainer, before something similar is proposed at work.

## What would settle it

Build an overview screen for one personal project and use it for a few weeks. Record each time the
screen led to an action, and whether that action needed a row that triage would not have shown (a
healthy resource, or logs that were not errors). If nothing on the screen led to an action that
triage would have missed, the answer is no, or "only as an orientation page".

Three sub-decisions follow if the answer is yes. Each needs its own record:

- whether the overview may fetch for itself, for a live log tail, which Decision 022 forbids for
  the TUI today
- whether hub may run actions against production (redeploy, roll back) directly, and with what
  confirmation, or only launch agents and open platform UIs as it does now
- whether the system description stays in `hub.toml` or moves into each repo, which changes
  Decision 003

## Resolves into

A decision record that amends the vision and Decision 001 to allow healthy state on a secondary
screen, plus the sub-decisions above. If the experiment says no, the finding goes into the vision's
"What hub is not" list.

## Source

Discussion on 2026-10-17 about adding an observability TUI to every project. It started as a
per-project Textual dashboard for a work system. That idea was set aside in favour of a folder of
agent skills, because work projects are short-lived and often run without instrumentation. The hub
version is the maintainer's own experiment with the format.

## Options

- **A project overview screen, rendered from `hub.toml`'s `[[project.environment]]` config.** It
  opens from a selected project or signal, and triage stays the only default screen. Each category
  (resource status, runtime logs, actions) is a module per platform that a project enables.
  Strongest case: it reuses hub's config, clients and daemon cache, and `hub.toml` already holds a
  partial description of each system (GCP project, Loki endpoint, Grafana URL, log queries). Cost:
  amends the vision, needs an exception to Decision 022 for live tails, and adds per-platform
  adapters (Vercel, Cloudflare, Fly, Kubernetes) beyond today's Loki and GCP clients.
- **No overview. Add health workflows instead.** Workflows emit signals for failed deploys, crashed
  machines and crash-looping pods, so broken resources reach triage. Strongest case: it covers "I
  didn't know it was broken" without changing hub's scope. Cost: no orientation view and no way to
  browse a system on purpose when nothing is flagged.
- **Hub launches a separate dashboard for each project**, the way it launches Claude Code in a tmux
  pane. Strongest case: hub's scope does not change, and a dashboard can serve people who don't use
  hub. Cost: each personal project needs its own dashboard, and the plugins hub would have shared
  get written per project.

## Findings

Nothing here is settled until it graduates into a decision record.

- A system overview is a second mode alongside triage ("what exists?" next to "what needs me?").
  Placing it on a secondary screen reached from a selected project keeps triage as the only default
  view. *Reasoned*
- The overview's value is lowest for a sole maintainer who already knows each system. It is highest
  for orientation after months away. *Reasoned*
- Per-repo dashboards were rejected for personal projects because each would rewrite the same
  category plugins. A shared multi-project tool was rejected for now because only one user and one
  codebase would use it, so plugin boundaries would be guesses. *Reasoned*
- The `[[project.environment]]` section of `hub.example.toml` already carries per-environment
  platform settings and log queries. *Sourced: hub.example.toml*
- Decision 022 says the TUI renders the cache and never fetches, which rules out a live log tail as
  written. *Sourced: docs/decisions/022-tui-reads-cache-never-fetches.md*
