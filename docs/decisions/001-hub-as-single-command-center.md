# 001 — Hub as the single command center

## Context

Three related projects existed in parallel:

- **hub** — a personal dashboard for things needing attention (GitHub PRs,
  production errors, etc.)
- **media-tools** — a daily digest for home server health
- **agency** — automated code maintenance loops (find problems → open
  issues → fix → open PRs)

All three were early sketches. Over time they would have converged on the
same core loop: observe signals from systems, understand what they mean,
decide what to do, act (or delegate to an agent), learn from outcomes.
They mainly differed in where the work started from.

## Decision

Hub is the single long-term investment. Media-tools and agency are
early explorations of the same problem from different angles — their
ideas fold into hub over time, not into separate products.

Hub's scope is broader than a dashboard. It is a place to **observe and
act** across any terrain I'm responsible for: software, infrastructure,
home systems, whatever. Output modes (dashboard, report, alert, issue,
agent action) are chosen based on what's appropriate for the signal — not
fixed to one format.

Agents are a first-class tool, not an afterthought. Where rule-based logic
is sufficient, use it. Where judgment, inference, or scale makes agents
more appropriate, use them. The line moves as the system matures.

## Consequences

- Home server integrations are a future addition to hub via
  hub-private — not a separate project to maintain.
- Agency-style loops (scan → triage → fix → PR) are a future capability
  of hub — not a separate product. The current agency-1 repo is a
  reference implementation to learn from, not something to merge directly.
- Hub is personal and single-user. A team-configurable or
  project-installable version of any of these capabilities is a different
  product with different design constraints — not a goal here.
- The "everything hub" failure mode (shows everything, becomes noise,
  stops getting checked) is the thing to design against. Ruthless
  prioritization is a first-class concern from the start.
