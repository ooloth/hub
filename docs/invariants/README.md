---
updated: 2026-09-16
update_when: never — this describes the format, not the invariants
decays: never
---

# Invariants

Facts about this system that have no sanctioned exception. Violating one means the system is
broken, not merely unconventional.

One file per invariant, named as the claim it makes, so a directory listing reads as the set of
things that must be true.

## How this differs from its siblings

[../decisions/](../decisions/) records choices that could have gone another way, and a decision can
be superseded by a later one. An invariant is what must hold *given* the decisions already taken.
Retiring one means reversing the decision underneath it, not granting an exception.

A standard is graded advice with legitimate exceptions, which is what `Should` and `Consider` mean
in `~/.agents/standards/`. Nothing here is graded. If a rule has a reasonable exception, it is a
standard and belongs there.

The test: could a competent person, knowing everything, decide to break this once for a good
reason? If yes, it is not an invariant.

## What a file contains

Four sections, in this order:

1. **The invariant** — one sentence, stated as a property of the system.
2. **Why it must hold** — the mechanism that breaks if it doesn't, concretely enough that a reader
   can picture the failure.
3. **What it forbids** — the specific things that violate it, including the ones that look
   reasonable. This is the section that earns its place; the invariant alone rarely rules out the
   tempting implementation by itself.
4. **How it is enforced** — the check that runs, and what that check does not cover.

## An invariant with no runner is a wish

The enforcement section is the point of the file. An invariant asserted in prose and checked by
nobody reads as settled while drifting freely, which is worse than not writing it down, because
the next reader trusts it.

So each one names the type, lint, test or script that holds it up — and names honestly what that
mechanism misses, since a partial check presented as a total one is the same failure one level up.
Where no check is possible, the file says so and says why, rather than leaving the section empty.
