# 005 — Where agent skills fit

## Context

Hub's observe→understand→act loop has two deterministic ends (fetching
data from external APIs, filing issues/alerts via external APIs) and a
judgment-based middle (interpreting what the data means, scoring
urgency, inferring what action is warranted). Rules handle the simple
cases; agents handle the cases that require judgment, inference, or
synthesis across multiple signals.

## Decision

Agent skills live in an `agents/` crate alongside `clients/`. The
Anthropic API is treated as another external service — `agents/` is
its adapter, the same way `clients/github/` adapts the GitHub API.

```
workflows/ → clients/github    # fetch (deterministic)
           → agents/classify   # understand (judgment-based)
           → clients/github    # act (deterministic)
```

Individual skills are named functions in `agents/` — each wraps a
prompt and returns structured output. Examples:

- `agents::classify::score_urgency(items) -> Vec<ScoredItem>`
- `agents::errors::group_traces(traces) -> Vec<ErrorGroup>`
- `agents::issues::draft_body(observation) -> String`

Keeping `agents/` separate from `clients/` makes the non-determinism
explicit: everything in `clients/` is deterministic and testable with
fixed inputs; everything in `agents/` involves LLM judgment and
requires different testing strategies (snapshot tests, evals).

## Consequences

- Workflows decide when to use an agent vs. pure logic. The threshold:
  use rules when the classification is clear and stable; use agents
  when judgment, context, or synthesis across signals is needed.
- `agents/` imports `domain/` for input/output types, same as
  `clients/`. It does not import `clients/` or `store/`.
- Agent calls are async, fallible, and slower than deterministic calls.
  Workflows that use them should handle degraded-mode gracefully (fall
  back to rule-based scoring if the LLM call fails).
- Skills are versioned with the hub codebase. Prompt changes are
  tracked in git alongside the code that calls them.
- Not all skills belong in hub. The distinction: **automation skills**
  (called programmatically by hub's workflows, hub-specific inputs and
  outputs) live in `agents/`. **Craft skills** (drafting, reviewing,
  analyzing — useful interactively across any project) live globally in
  `~/.claude/skills/` and are honed there independently of hub. Hub
  can invoke global skills too; the point is that general-purpose
  skills shouldn't be locked inside hub's codebase.
