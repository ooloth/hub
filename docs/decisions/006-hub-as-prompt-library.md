# 006 — Hub as a Claude Code investigation prompt library

## Context

Hub's `hub.toml` knows things that project-specific Claude Code agents
don't: which Loki endpoint serves a project's production logs, which
LogQL query selects the right app, what the project is called, which
environment is prod vs staging. An investigation that has to ask for
this context is slower and more error-prone than one that can read it
from hub's config.

At the same time, interactive investigations (multi-turn Claude sessions
that query external APIs iteratively) don't belong in the `agents/`
crate — they're conversations, not function calls. They aren't general
craft skills either: they're hub-specific, reading hub's config, and
only useful in the context of hub's projects.

Project-specific investigation prompts (added to each project's own
repo) solve the immediate problem but create a proliferation problem:
the same prompt reimplemented in every repo, each slightly different,
with hardcoded endpoints and no shared config model. When you add a new
project to hub, you also have to add the prompt to that repo.

## Decision

Hub's repo houses a library of investigation prompts in `prompts/`.
These prompts:

- Read their configuration (endpoint, query, project name, environment)
  from `hub.toml` context available at runtime
- Use external CLI tools (`logcli`, `gh`, `curl`, etc.) to fetch data
  iteratively
- Produce diagnoses, summaries, recommendations, or filed issues
- Live alongside hub's Rust code, versioned with it, but are distinct
  from the `agents/` crate

Prompts are launched via `claude --system-prompt "$(cat prompts/<name>.md)" "<task>"` —
either from the TUI (embedded at compile time via `include_str!`) or from
a scheduled `claude -p` run. This avoids slash-command discovery, which
requires the skills directory to be present in the working tree.

This is a third category of agent capability, distinct from:

| Category | Location | Mode | Who runs it |
|---|---|---|---|
| Background automation | `agents/` crate (planned, not yet built) | Single API call, structured output | Hub workflows, unattended |
| Hub investigation prompts | `prompts/` in hub's repo | Multi-turn, iterative | TUI keypress or scheduled run |
| General craft skills | `~/.claude/skills/` globally | Multi-turn, general-purpose | Human, any project |

## Consequences

- Hub's `prompts/` directory is a first-class artifact,
  maintained alongside `clients/` and `workflows/`.
- Prompts are added via the `add-a-prompt` playbook, not the
  `add-a-workflow` playbook.
- Prompts receive hub.toml config via the task string (scheduled runs)
  or via the TUI item context (TUI investigations).
- The same prompt works for any project configured in `hub.toml` — no
  per-project duplication.
- Hub's investigation capability does not require the `agents/` crate
  and does not block on it. Prompts can be added before `agents/` exists.
- A prompt that proves durable and valuable — something that runs daily
  unattended — is a candidate for later promotion to `agents/`
  automation. That promotion is a deliberate step, not assumed.
- This is one of hub's core hypotheses: that centralizing project
  config in `hub.toml` makes agentic investigation faster to launch
  and more contextually accurate than navigating to each project repo
  and running an investigation there.
