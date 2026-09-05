---
updated: 2026-09-05
update_when: never — this describes the format, not the decisions
decays: never
---

# Decisions

A record of the reasoning behind choices that took real thought, or could reasonably have gone a
different way. Sibling of [../questions/](../questions/): a question moves here once it's answered.

One file per decision, `NNN-kebab-title.md`, numbered in the order written — append-only, never
renumbered (no tool here checks the broken links a renumber would create, so don't rely on being
able to fix them all).

**A decision that changes is superseded by a new record, not edited into a different one.** What
was believed at the time survives, even once it's wrong. Fixing a wrong figure, a broken link, or
an unclear sentence is still just an edit — the test is whether the reasoning would read the same
to someone who disagreed with it. Note a substantive amendment with an `amended:` date in
frontmatter, or — matching existing practice here — a status line under the title (e.g. `> Status:
Superseded by Decision 012`) and/or an `## Addendum (YYYY-MM-DD) — topic` section appended at the
end.

## What doesn't go here

A record captures an **architectural** decision: one that changes the system's boundaries or
constrains long-term technical options. Plenty of things get settled in the course of the work
without meeting that bar, and they are not records however much discussion they took.

Two that look like candidates and aren't:

- **A reversible feature nuance.** How many items a notification names, whether a list backfills
  immediately or waits for the next pass. Reversing it later costs an afternoon and moves no
  boundary.
- **A choice settled by trying it.** Where building is cheaper than arguing, build it — the code
  is the record, and a spike that produced an observation belongs in a question's **Findings**.

The test: reversing this later, would someone have to revisit a boundary, or just edit a value?

Promoting a nuance anyway is not a harmless excess. Everything written after a record treats it as
settled, so a record nobody needed constrains work that had no reason to be constrained. These
still need somewhere to live — record them in the issue that implements the behaviour, and see
[../questions/README.md](../questions/README.md) for closing the question that raised them.

## Before you decide

Start with a listing of this folder — every title states what's now true, so the listing is the
checklist of settled constraints. Read `vision.md` too: promises and direction stated there bind as
hard as anything here and won't show up in this listing.

Invoke `uphold-standards` before deciding, even if already read this session — a remembered summary
produces a decision that fits the format and breaks a rule.

## One record, one decision

A record settles exactly one thing, and its title says what. Test: could a reasonable person have
decided the headline one way and the second thing the other way? If yes, that's two decisions —
two files. A consequence that follows necessarily from an earlier record still gets its own file:
it's a real constraint, and invisible if it only lives inside another record's reasoning (see
[010](010-hub-cli-as-agent-toolkit.md)'s addendum — a boundary that had to be stated explicitly
once a related decision made it relevant). Where there's no genuine alternative, **Rejected** says
so and names what rejecting it would actually mean, rather than inventing an option to fill the
section.

Don't fear a long listing — atomic records multiply, and that's the cost of a folder whose
filenames are load-bearing. A hundred short titles beat twelve long records you have to read.

## The title is the takeaway

A title states what is now binding, not why it was decided or what option it preserves — the
motive belongs in **Forced by**, the option preserved belongs in **Decision**. If a title doesn't
tell a reader whether to open the file, rename it.

## You arrive with a leaning

Assume you already favor something before evaluating anything — familiarity, or whichever option
you read first. That's not the problem; not knowing it is, because it quietly turns evaluation into
a search for reasons.

Write the strongest case *for* each option you expect to reject, before writing the case against
any of them — the order is the mechanism. The tell it went wrong: every weak reason points the same
direction. If the exercise changed nothing, say so plainly — either the decision was genuinely
obvious, or the alternatives were performed rather than considered.

Each option is evaluated from first principles: no claim carried over without re-establishing it,
no number without its source. One disqualifying reason per rejected option, named — not a stack of
weak ones; if none would disqualify it alone, it isn't disqualified. Every rejection says what would
have to change for it to reverse.

Check [../questions/README.md](../questions/README.md) for the question this answers, and confirm
nothing it names as still-open is actually load-bearing here — a decision resting on an unsettled
question will read as confident without being so.

## Size the decision first

A template captures a decision; it doesn't improve one. How expensive is this to reverse?
Cheap-to-reverse: pick one and move — a coin toss deserves a coin toss. For the expensive ones:

1. State the problem without naming a solution — a lot of false choices dissolve right here.
2. Estimate the magnitudes (how much, how often, how large, how fast). Most bad technical
   decisions skip the arithmetic. Can't get within an order of magnitude? Go measure — that's the
   finding.
3. Ask what's cheaper to build than to argue about. A spike (the smallest throwaway thing that
   produces an observation) usually settles a tool/runtime question better than reading about it.
4. Find three options; make one of them "not yet." Two options is a coin toss with extra steps.
5. Predict each option's failure mode, and whether it fails loudly or silently. Loud beats quiet
   even when otherwise worse.
6. Write down what would change your mind before deciding — this becomes **Revisit when**.

Decide one thing at a time, while looking at everything it touches — a decision about where
something runs isn't a decision about what it stores, even when one rides along inside the other.
Name what else the choice moves before recording it. Familiarity is a cost to state against the
alternative, never a merit smuggled in for the option you already know.

## Cite, don't restate

**Forced by** references a specific finding, file, or measurement — not a restated vague standard.
An ADR citing nothing was made on vibes.

Records 001–019 predate this template and use a looser `Context` / `Decision` / `Consequences`
shape with no frontmatter — they aren't being retrofitted. The template below applies from here
on.

## Template

```
---
number: NNN
status: proposed | accepted | superseded by NNN
date: YYYY-MM-DD
---

# NNN — <the choice, plainly stated as a conclusion>

## Forced by
<the constraint, need, or finding that made this necessary — by reference>

## Decision
<what we're doing, and briefly why the alternatives lost>

## Rejected
- <Option A> — because <the actual disqualifying reason>
- <Option B> — because <...>

## Risk
<the real cost or weakness being knowingly accepted>

## Revisit when
<the observable condition that should trigger reconsidering this>

## Also update
- [ ] questions/README.md — which question this settles or re-scopes
- [ ] vision.md — anything this decision defines or forecloses
```

## Guidance

- **Rejected** needs the actual disqualifying reason, not a bare label — "considered X" tells a
  reader nothing; "considered X, rejected because Y" does.
- **Risk** keeps a record honest rather than a justification. Nothing being knowingly accepted
  usually means the decision was trivial, or the risk hasn't been found yet.
- **Revisit when** names an observable condition, not a date — otherwise every record reads as
  equally binding forever.
- An unchecked box under **Also update** is visibly unfinished work.
