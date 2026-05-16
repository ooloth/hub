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

> **Note:** The `agents/` crate section of this decision is superseded
> by [Decision 009](009-no-scheduled-runs.md). The `agents/` crate will
> not be built. All LLM interaction is handled via interactive prompts
> launched from the TUI.

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

- Investigation prompts in `prompts/` are launched by the TUI — not
  typed by the user as slash commands. They are conversations, not
  function calls.
- Craft skills (drafting, reviewing, analyzing — useful interactively
  across any project) live globally in `~/.claude/skills/` and are
  honed independently of hub. They are not hub-aware.
