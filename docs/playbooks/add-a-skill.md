# Add a Skill

Steps to add a Claude Code investigation skill to hub. Skills are
multi-turn conversations where Claude uses CLI tools to query data
iteratively, guided by a human question. They differ from workflows:
workflows run deterministically and emit ranked items; skills run
interactively and produce diagnoses.

See [Decision 006](../decisions/006-hub-as-skill-library.md) for the
full model and rationale.

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

## 3. Register new config fields in the schema (if needed)

If the skill reads hub.toml fields that don't already exist in the
schema, add them to `config/schemas/hub.toml.schema.json` so editors
can validate and autocomplete them. Follow the pattern of existing
workflow definitions.

## 4. Write the skill file

Create `.claude/skills/<name>.md`. A skill file contains:

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

## 6. Note in vision.md (for new skill categories)

If the skill opens a new category of investigation (e.g. the first log
investigation skill, the first infrastructure skill), add a sentence to
the Investigation section in `docs/vision.md`.

## Private skills

If the skill requires private config (endpoints, queries that reveal
internal infrastructure), add it to `hub-private` instead:
`.claude/skills/` in the hub-private repo, symlinked into hub's
`.claude/skills/` directory. Follow the same pattern as private
workflows.
