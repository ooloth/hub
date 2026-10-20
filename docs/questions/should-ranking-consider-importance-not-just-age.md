---
opened: 2026-09-04
status: open
resolves_into: decision
---

# Should turns be ranked by importance, not just age?

## Why it matters

Each pull request that is my turn gets its own card, so notifications take no order from ranking.
Ranking matters where turns are read in order: the list, which sorts every signal by urgency tier,
then age (`sort_by_key(|i| (i.urgency(), Reverse(i.age())))` in `workflows/src/status.rs`), and the
order by how long each has been my turn that #374 adds. A genuinely more important turn that is
newer sits below older ones under age alone. Whether that is a real problem worth solving, or an
acceptable trade-off for an order that stays predictable, isn't yet known.

## What would settle it

A concrete case, after living with oldest-first for a while, where an urgent turn sat unaddressed
because older, less important turns were listed above it.

## Resolves into

`../decisions/` — a new ADR, if and when this is picked up.

## Source

Raised during the PR-notification-and-auto-launch design discussion (in `ooloth/dotfiles`,
2026-09-04), migrated here once the decision to build in hub was made. See the epic issue for full
context.

## Options

- **A. Oldest-first only.** Current plan. Matches the existing sort's secondary key; no new
  ranking logic needed.
- **B. Importance-first, oldest as tiebreak.** Already partially true of the existing sort
  (`urgency` is the primary key). The open question is whether turns should lean on that more, or
  ignore it in favor of pure age to keep the order predictable.

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it
graduates into a decision record._

- *Reasoned* (2026-09-19): milestone #327's Phase 6 ranks the list by time since the ball landed on
  me, which is option A applied to a better age measure. It does not decide whether importance
  should outrank age, so this question survives the milestone and the milestone is what produces
  its evidence.
- *Stated* (2026-10-19, the maintainer): actionable items rank above non-actionable ones through
  the shared urgency tiers, which are expected to grow longer and more granular, rather than
  through per-source sorting. Each source assigns a tier from the item's state; the list sorts
  every signal by tier, then age.
