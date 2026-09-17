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

> **⚠ Superseded by [Decision 012](012-task-model.md), which
> [Decision 019](019-drop-task-model-filesystem-sessions.md) then dropped.** 012
> routed all delegated work through tasks dispatched by a `hub` CLI polling
> loop. 019 removed the task model, so the model described below is the one that
> runs: a TUI keypress launches a Claude session in a tmux pane, with the prompt
> embedded by `include_str!`. See `ui/tui/src/investigations/`.
>
> The `agents/` crate is the exception, and it is not built.
> [Decision 009](009-no-scheduled-runs.md) cancelled it and nothing has revived
> it: the Cargo workspace has no `agents` member.
> [Decision 020](020-hub-runs-an-unattended-surface.md) adds an unattended
> surface, but that surface makes no Claude calls, so it is not this crate.

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
