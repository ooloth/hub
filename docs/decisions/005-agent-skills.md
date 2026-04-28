# 005 — Where agent skills fit

## Context

Hub's observe→understand→act loop has two deterministic ends (fetching
data from external APIs, filing issues/alerts via external APIs) and a
judgment-based middle (interpreting what the data means, scoring
urgency, inferring what action is warranted). Rules handle the simple
cases; agents handle the cases that require judgment, inference, or
synthesis across multiple signals.

There are two fundamentally different kinds of agent capability, and
they must not be conflated:

- **Background automation** — unattended, runs as part of a workflow,
  single API call, structured output. Hub calls the LLM the same way
  it calls any external API.
- **Interactive investigation** — human in the loop, multi-turn,
  iterative querying. Claude makes N queries, observes output, forms a
  hypothesis, queries again. A single API call cannot replicate this;
  it is a conversation, not a function.

## Decision

### Background automation → `agents/` crate

Automation skills live in an `agents/` crate alongside `clients/`. The
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

### Interactive investigation → Claude Code skills in hub's repo

Investigation skills live in hub's `.claude/skills/` directory. These
are Claude Code skills — multi-turn conversations where Claude uses CLI
tools (`logcli`, `gh`, etc.) to query data iteratively, form
hypotheses, and validate them. A Rust function calling the API once
cannot replicate this loop.

Hub's unique contribution to these skills is **context**. Hub knows
(from `hub.toml`) which Loki endpoint serves a project's production
logs, which LogQL query selects the right app, what the project is
called. A skill that reads this context requires zero user setup to
invoke correctly.

```
hub status                  # surfaces: "prod: 12 errors in last hour"
claude /loki-investigate    # investigates: reads hub.toml for endpoint
                            # + query, then iterates until diagnosed
```

See [Decision 006](006-hub-as-skill-library.md) for the full model.

## Consequences

- Automation skills in `agents/` are called by workflows, run
  unattended, and must handle degraded mode gracefully (fall back to
  rule-based logic if the LLM call fails).
- `agents/` imports `domain/` for input/output types, same as
  `clients/`. It does not import `clients/` or `store/`.
- Investigation skills in `.claude/skills/` are invoked by the user,
  not by workflows. They are conversations, not function calls.
- Craft skills (drafting, reviewing, analyzing — useful interactively
  across any project) live globally in `~/.claude/skills/` and are
  honed independently of hub. They are not hub-aware.
- A skill that proves durable and valuable as an interactive
  investigation is a candidate for later promotion to `agents/`
  automation — but that promotion is a deliberate step, not assumed.
