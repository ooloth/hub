---
opened: 2026-09-04
status: open
resolves_into: decision
---

# Should the active notified set backfill immediately as items clear, or wait for the next detection pass?

## Why it matters

The active set (top-N) is deliberately smaller than the full backlog, to drive completion rather
than overwhelm. When an item clears, two behaviors are possible: shrink in place (3→2→1→0, no
backfill until the next scheduled detection pass) or immediately pull in the next-highest-priority
backlog item to keep the set topped up. Shrink-in-place is simpler and favors finishing active work
over continuously re-grooming the list (a deliberate, kanban-style WIP-limiting choice) — the
trade-off is that a more important item appearing mid-cycle waits for the next pass rather than
being surfaced right away.

## What would settle it

Living with shrink-in-place for a while and finding a concrete case where waiting for the next
detection pass actually cost something, versus it simply not mattering in practice.

## Resolves into

`../decisions/` — a new ADR, if and when this is picked up.

## Source

Raised during the PR-notification-and-auto-launch design discussion (in `ooloth/dotfiles`,
2026-09-04), migrated here once the decision to build in hub was made. See the epic issue for full
context.

## Options

- **A. Shrink in place, no backfill until the next detection pass.** Chosen for the milestone.
  Simpler; no mid-cycle re-ranking logic needed.
- **B. Immediate backfill on completion.** Keeps the active set always at N, but reintroduces the
  "important item interrupts what I'm already focused on" dynamic the shrink-in-place choice was
  meant to avoid.

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it
graduates into a decision record._

...
