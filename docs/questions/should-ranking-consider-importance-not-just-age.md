---
opened: 2026-09-04
status: open
resolves_into: decision
---

# Should the notified set be ranked by importance, not just age?

## Why it matters

Oldest-first is the v1 ranking rule for the notified top-N — simple, and it's already what
`workflows/src/status.rs` computes today (`sort_by_key(|i| (i.urgency(), Reverse(i.age())))`, so
urgency already outranks age in the existing sort). A genuinely more important item that's newer
than everything else in the current top-N has to wait for the next detection pass under the
shrink-in-place design. Whether that's a real problem worth solving, versus an acceptable trade-off
for the simplicity of "finish what's active before grooming the list again," isn't yet known.

## What would settle it

A concrete case, after living with oldest-first for a while, where a genuinely urgent item sat
unaddressed specifically because the active set was already full of older, less important items.

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
  (`urgency` is the primary key) — the open question is really whether the top-N selection for
  *this* milestone should lean on that more, or deliberately ignore it in favor of pure age to keep
  the "shrink in place, favor finishing" behavior predictable.

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it
graduates into a decision record._

...
