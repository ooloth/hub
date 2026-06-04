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

> **⚠ Superseded by [Decision 012](012-task-model.md).** The investigation
> prompt model described here — TUI keypress launches a Claude session in a
> tmux window — is replaced by the task system. All delegated work, including
> investigations, now flows through tasks dispatched via the `hub` CLI polling
> loop. Skills still live in hub's repo and provide agents with zero-setup
> context, but they are invoked through task dispatch, not direct TUI keypresses.
> The `agents/` crate section was already superseded by Decision 009.

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

Prompts are launched via TUI keypress (embedded at compile time via
`include_str!` and passed as `--system-prompt` to a tmux split). This
avoids slash-command discovery, which requires the skills directory to
be present in the working tree of whatever project is being investigated.

See [Decision 006](006-hub-as-prompt-library.md) for the full model.

## Consequences

- Investigation prompts in `prompts/` are launched by the TUI — not
  typed by the user as slash commands. They are conversations, not
  function calls.
- Craft skills (drafting, reviewing, analyzing — useful interactively
  across any project) live globally in `~/.claude/skills/` and are
  honed independently of hub. They are not hub-aware.
