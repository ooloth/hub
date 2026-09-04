---
opened: 2026-09-04
status: open
resolves_into: decision
---

# Where do auto-triggered investigation outputs appear, and are they worth building?

## Why it matters

The PR-notification milestone's core scope is a *convenient, manually-triggered* investigation
(the user presses a key or command, a session opens with a system-authored prompt) — not a session
that starts investigating an item on its own before anyone asks. Auto-triggering was floated as
"could be cool" but explicitly deferred: it isn't clear where its output should surface (a badge on
the signal row before anyone's looked? a separate inbox? does it risk `vision.md`'s trust-gated
scaling principle — "machine-initiated dispatch... follows earned trust, never the reverse"?).

## What would settle it

Using the manually-triggered version for a while and observing whether the friction of pressing a
key per item is actually a problem worth solving, versus whether "convenient to trigger" already
removes the pain. If it does turn out to matter, a concrete answer to where the output goes before
a human has looked at it.

## Resolves into

`../decisions/` — a new ADR, if and when this is picked up. Likely needs to reference
`vision.md`'s trust-gated scaling section directly, since auto-triggering without review is exactly
the thing that section argues against doing before the flywheel has proven itself.

## Source

Raised during the PR-notification-and-auto-launch design discussion, 2026-09-04, explicitly
deferred. See the epic issue for the full context.

## Options

...

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it
graduates into a decision record._

...
