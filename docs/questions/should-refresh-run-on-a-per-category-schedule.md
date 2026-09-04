---
opened: 2026-09-04
status: open
resolves_into: decision
---

# Should refresh run on a per-category schedule instead of one global interval?

## Why it matters

Today `REFRESH_INTERVAL_SECS` (`ui/tui/src/main.rs:35`) is a single 30-minute timer for every
signal category. Some categories plausibly need fresher data than others (a PR reply probably
matters sooner than a CI failure that's already been open for a day), and running everything on
one clock means either over-fetching the categories that don't need it or under-fetching the ones
that do. Getting this right was raised as one plausible (unconfirmed) lever on why hub stopped
being opened regularly — stale-feeling data erodes trust in the single-pane-of-glass premise.

## What would settle it

Evidence that a single global interval is actually causing either wasted fetches or stale-feeling
data for a specific category — not just the intuition that categories differ. Ideally: after the
PR-notification milestone ships and gets used for a while, whether staleness (rather than "I forgot
to open it") shows up as an actual complaint.

## Resolves into

`../decisions/` — a new ADR, once there's real evidence a shared interval is the problem rather
than a lower-priority polish item.

## Source

Raised during the PR-notification-and-auto-launch design discussion, 2026-09-04. Deliberately
deferred out of that milestone's scope — see the epic issue for the full context.

## Options

- **A. Keep one global interval.** Simplest; status quo.
- **B. Per-category intervals**, each category owning its own timer.
- **C. Per-category *staleness thresholds*** rather than fixed timers — refetch a category only
  when its data is older than a category-specific threshold, checked opportunistically rather than
  on N independent clocks. Not yet compared against B for implementation cost.

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it
graduates into a decision record._

...
