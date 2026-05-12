# 005 — The `agents/` crate and `prompts/`

## Context

Hub's observe→understand→act loop has two deterministic ends (fetching
data from external APIs, filing issues/alerts via external APIs) and a
judgment-based middle (interpreting what the data means, scoring
urgency, inferring what action is warranted). Rules handle the simple
cases; LLM calls handle the cases that require judgment, inference, or
synthesis across multiple signals.

There are two fundamentally different kinds of LLM call, and they must
not be conflated:

- **Background automation** — unattended, runs as part of a workflow,
  single API call, structured output. Hub calls the LLM the same way
  it calls any external API.
- **Interactive investigation** — human in the loop, multi-turn,
  iterative querying. Claude makes N queries, observes output, forms a
  hypothesis, queries again. A single API call cannot replicate this;
  it is a conversation, not a function.

## Decision

### Background automation → `agents/` crate (planned, not yet built)

The intended home for background automation is an `agents/` crate
alongside `clients/`. The Anthropic API would be treated as another
external service — `agents/` would be its adapter, the same way
`clients/github/` adapts the GitHub API.

```
workflows/ → clients/github    # fetch (deterministic)
           → agents/classify   # understand (judgment-based)
           → clients/github    # act (deterministic)
```

Individual functions in `agents/` would each wrap a prompt and return
structured output. Examples:

- `agents::classify::score_urgency(items) -> Vec<ScoredItem>`
- `agents::errors::group_traces(traces) -> Vec<ErrorGroup>`
- `agents::issues::draft_body(observation) -> String`

Keeping `agents/` separate from `clients/` makes the non-determinism
explicit: everything in `clients/` is deterministic and testable with
fixed inputs; everything in `agents/` involves LLM judgment and
requires different testing strategies (snapshot tests, evals).

**The `agents/` crate has not been built yet.** Background automation
will be added when a concrete use case warrants it.

### Interactive investigation → `prompts/`

Investigation prompts live in hub's `prompts/` directory as plain
markdown files. They are multi-turn conversations where Claude uses CLI
tools (`logcli`, `gh`, etc.) to query data iteratively, form
hypotheses, and validate them. A Rust function calling the API once
cannot replicate this loop.

Hub's unique contribution is **context**. Hub knows (from `hub.toml`)
which Loki endpoint serves a project's production logs, which LogQL
query selects the right app, what the project is called. A prompt that
reads this context requires zero user setup to invoke correctly.

Prompts are launched two ways: via TUI keypress (the prompt is embedded
at compile time via `include_str!` and passed as `--system-prompt` to a
tmux split) or via scheduled `claude -p` run. This avoids slash-command
discovery, which requires the skills directory to be present in the
working tree of whatever project is being investigated.

See [Decision 006](006-hub-as-prompt-library.md) for the full model.

## Consequences

- When built, functions in `agents/` will be called by workflows, run
  unattended, and must handle degraded mode gracefully (fall back to
  rule-based logic if the LLM call fails).
- `agents/` will import `domain/` for input/output types, same as
  `clients/`. It will not import `clients/` or `store/`.
- Investigation prompts in `prompts/` are launched by the TUI or by a
  scheduler — not typed by the user as slash commands. They are
  conversations, not function calls.
- Craft skills (drafting, reviewing, analyzing — useful interactively
  across any project) live globally in `~/.claude/skills/` and are
  honed independently of hub. They are not hub-aware.
- A prompt that proves durable and valuable as an interactive
  investigation is a candidate for later promotion to `agents/`
  automation — but that promotion is a deliberate step, not assumed.
