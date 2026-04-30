# Add a Skill

Steps to add a Claude Code investigation skill to hub. Skills are
multi-turn conversations where Claude uses CLI tools to query data
iteratively, guided by a human question. They differ from workflows:
workflows run deterministically and emit ranked items; skills run
interactively and produce diagnoses.

See [Decision 006](../decisions/006-hub-as-skill-library.md) for the
full model and rationale.

## Automation compatibility

All skills in this project must work without user input. A skill may
be invoked interactively by a human, or autonomously by a scheduled
run — it cannot know which at runtime, and blocking on a prompt will
hang an automated run indefinitely.

**Rules:**

- Never ask the user a question or wait for a response
- If the invocation is ambiguous, make a best-effort interpretation
  and log what was chosen; do not stop to confirm
- If required information is missing (e.g. no theme given), apply a
  sensible default (e.g. all opted-in themes) and log it
- If something is unrecognised, skip it with a logged warning and
  continue; do not abort the run

The scope echo pattern — printing a one-line summary of the
interpreted scope before doing any work — is encouraged as a
transparency mechanism, but it must not block: print it and proceed.

## 1. Understand what kind of skill this is

Skills in hub's `.claude/skills/` directory are **hub investigation
skills** — they:

- Read configuration from hub.toml context (endpoint, query, project,
  environment) that the user loaded before invoking the skill
- Use external CLI tools (`logcli`, `gh`, `curl`, etc.) to query data
- Iterate — form a hypothesis, query to validate, refine, query again
- Produce a human-readable output (table, summary, diagnosis,
  recommendation)

They are **not**:

- `agents/` crate functions (those are single-call background
  automation, unattended)
- Global craft skills (those live in `~/.claude/skills/`, are
  general-purpose, and are not hub-aware)

## 2. Identify what config the skill needs

List the hub.toml fields the skill will read. Keep this minimal —
only what the skill genuinely needs to avoid asking the user.

Example for a Loki investigation skill:

```toml
[[project.environment.workflow]]
name = "loki-logs"
endpoint = "https://loki.example.com"
query = '{app="my-app", env="prod"}'
```

The skill references these as named values in its prompt context.

## 3. Register new config fields (if needed)

If the skill introduces a new `[[project.workflow]]` name (e.g.
`repo-scan-docs`), follow steps 5–7 of
[Add a Workflow](add-a-workflow.md): Rust enum variant, JSON schema
definition, and hub.toml.example entry. All three are required.

## 4. Write the skill file

Create `.claude/skills/<name>/SKILL.md` (a subdirectory containing
`SKILL.md` — flat `.md` files are not discovered by Claude Code).

The file must begin with YAML frontmatter followed by the skill body:

```markdown
---
name: <name>
description: <one sentence — shown in /skill-name autocomplete>
allowed-tools: [Bash]
effort: high
model: opus
---

## Purpose
...
```

The body contains:

1. **Purpose** — one sentence on what question this skill answers
2. **Prerequisites** — CLI tools needed (`logcli`, `gh`, etc.) and how
   to install them
3. **Config** — which hub.toml fields it reads and what they mean
4. **Starting queries** — the first one or two queries to orient
   Claude, before it begins iterating on its own
5. **Investigation pattern** — how Claude should form hypotheses and
   validate them (when to go deeper, when to stop, what counts as a
   good answer)
6. **Output format** — what the final answer should look like (table,
   paragraph summary, ranked list, etc.)

## 5. Update the hub.toml example

Add an example entry to `hub.toml.example` showing the config fields
the skill reads, under the appropriate section
(`[[project.workflow]]`, `[[project.environment.workflow]]`, or
`[[monitor.workflow]]`).

## 6. Test the skill

Run the skill non-interactively from hub's repo directory:

```bash
claude -p --dangerously-skip-permissions /repo-scan docs hub
```

This is the same invocation path used by automated runs — no interactive
session, output to stdout, no prompts. If it works here, it will work
unattended. Pass a project name to override hub.toml scope filtering
so you can test without opting the project in first.

The `--dangerously-skip-permissions` flag is required if your personal
settings enable plan mode — without it the skill will pause waiting for
approval and hang an automated run.

## 7. Note in vision.md (for new skill categories)

If the skill opens a new category of investigation (e.g. the first log
investigation skill, the first infrastructure skill), add a sentence to
the Investigation section in `docs/vision.md`.

## Private skills

If the skill requires private config (endpoints, queries that reveal
internal infrastructure), add it to `hub-private` instead:
`.claude/skills/` in the hub-private repo, symlinked into hub's
`.claude/skills/` directory. Follow the same pattern as private
workflows.
