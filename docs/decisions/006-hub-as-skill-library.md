# 006 — Hub as a Claude Code skill library

## Context

Hub's `hub.toml` knows things that project-specific Claude Code skills
don't: which Loki endpoint serves a project's production logs, which
LogQL query selects the right app, what the project is called, which
environment is prod vs staging. A skill that has to ask the user for
this context is slower and more error-prone than one that can read it
from hub's config.

At the same time, interactive investigation skills (multi-turn Claude
Code sessions that query external APIs iteratively) don't belong in
the `agents/` crate — they're conversations, not function calls, and
they require a human in the loop. They aren't general craft skills
either: they're hub-specific, reading hub's config, and only useful in
the context of hub's projects.

Project-specific skills (added to a project's own repo) solve the
immediate problem but create a proliferation problem: the same skill
reimplemented in every repo, each slightly different, with hardcoded
endpoints and no shared config model. When you add a new project to
hub, you also have to add the skill to that repo.

## Decision

Hub's repo houses a library of Claude Code investigation skills in
`.claude/skills/`. These skills:

- Read their configuration (endpoint, query, project name, environment)
  from `hub.toml` context that was loaded before the session started
- Use external CLI tools (`logcli`, `gh`, `curl`, etc.) to fetch data
  iteratively
- Produce diagnoses, summaries, or recommendations for the human to
  act on
- Live alongside hub's Rust code, versioned with it, but are distinct
  from the `agents/` crate

This is a third category of agent capability, distinct from:

| Category | Location | Mode | Who runs it |
|---|---|---|---|
| Background automation | `agents/` crate | Single API call, structured output | Hub workflows, unattended |
| Hub investigation skills | `.claude/skills/` in hub's repo | Multi-turn, iterative | Human, on demand |
| General craft skills | `~/.claude/skills/` globally | Multi-turn, general-purpose | Human, any project |

## Consequences

- Hub's `.claude/skills/` directory is a first-class artifact,
  maintained alongside `clients/`, `workflows/`, and `agents/`.
- Skills are added via the `add-a-skill` playbook, not the
  `add-a-workflow` playbook.
- Skills read hub.toml config via context that the user has loaded
  before invoking the skill — they do not read the TOML file at
  runtime.
- The same skill works for any project configured in `hub.toml` — no
  per-project duplication.
- Hub's investigation capability does not require the `agents/` crate
  and does not block on it. Skills can be added before `agents/` exists.
- A skill that proves durable and valuable — something you invoke
  manually every day — is a candidate for later promotion to `agents/`
  automation. That promotion is a deliberate step, not assumed.
- This is one of hub's core hypotheses: that centralizing project
  config in `hub.toml` makes agentic investigation faster to launch
  and more contextually accurate than navigating to each project repo
  and running a skill there.
