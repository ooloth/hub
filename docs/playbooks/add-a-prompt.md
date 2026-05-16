# Add a Prompt

## Should you add this?

Hub investigation prompts are instruction files for Claude — multi-turn
conversations where Claude uses CLI tools to query data iteratively and
produces a diagnosis or action. They differ from workflows: workflows run
deterministically and emit ranked items; prompts run as Claude agents and
produce diagnoses, recommendations, or filed issues.

Hub investigation prompts live in `prompts/` as plain markdown
with no frontmatter. They are launched as tmux splits when the user
presses a key on a TUI item (embedded at compile time via `include_str!`
in `ui/tui/src/investigations/`). Context comes from the selected item.

They are **not**:

- Global craft skills (those live in `~/.claude/skills/`, are
  general-purpose, and are not hub-aware)

See [Decision 006](../decisions/006-hub-as-prompt-library.md) for the
original model and rationale.

## 1. Identify what context the prompt needs

List what the prompt requires to run. All context comes from the TUI
item (repo slug, issue number, error message, etc.) — these fields are
formatted into the task string by the Rust investigation module.

Keep context minimal — only what Claude genuinely needs to avoid asking.

## 2. Register new config fields (if needed)

If the prompt reads a new `[[project.workflow]]` name from hub.toml,
follow steps 5–7 of [Add a Workflow](add-a-workflow.md): Rust enum
variant, JSON schema definition, and hub.toml.example entry. All three
are required.

## 3. Write the prompt file

Create `prompts/<name>.md` — a plain markdown file, no frontmatter.

The file should contain:

1. **Purpose** — one sentence on what question this prompt answers
2. **Prerequisites** — CLI tools needed (`gh`, `logcli`, `curl`, etc.)
   and how to install them
3. **Context** — what fields arrive in the task string and what they mean
4. **Starting queries** — the first one or two queries to orient
   Claude before it begins iterating
5. **Investigation pattern** — how Claude should form hypotheses and
   validate them (when to go deeper, when to stop, what counts as done)
6. **Output format** — what the final answer looks like (table, summary,
   ranked list, filed issues, etc.)

See existing prompts in `prompts/` for examples.

## 4. Wire into the TUI (if TUI-launched)

If the prompt is triggered by pressing `i` on a TUI item:

1. Add `include_str!("../../../../prompts/<name>.md")` and a
   `config(...)` function in a new `ui/tui/src/investigations/<name>.rs`
   module, following the pattern in `ci.rs` or `issue.rs`.
2. Declare the module in `ui/tui/src/investigations/mod.rs`.
3. Add an `InvestigationKind::<Name>` variant to `ui/tui/src/display.rs`
   and map the relevant `StatusItem` variant to it in
   `item_investigation()`.
4. Add `InvestigateAction::Launch<Name>` and `Effect::Launch<Name>`
   variants in `ui/tui/src/state/types.rs`.
5. Wire the action and effect in `ui/tui/src/state/update.rs` and
   `ui/tui/src/main.rs`.

## 5. Update hub.toml.example (if needed)

If the prompt reads new hub.toml fields, add an example entry to
`hub.toml.example` showing those fields under the appropriate section.

## 6. Test the prompt

Follow the Tier 2 tmux E2E instructions in `ui/tui/README.md` to drive
the keypress and verify the split launches correctly.

## 7. Note in vision.md (for new investigation categories)

If the prompt opens a new category of investigation (e.g. the first
infrastructure prompt), add a sentence to the Investigation section in
`docs/vision.md`.

## Private prompts

If the prompt requires private config (endpoints or queries that reveal
internal infrastructure), add it to `hub-private` instead:
`prompts/` in the hub-private repo, with a gitignored symlink in
hub's `prompts/`. Add the symlink creation to
`scripts/setup-private.sh`. See the media investigation as an example.

## Done when

- The prompt runs to completion non-interactively and produces a useful
  output
- `hub.toml.example` has been updated if the prompt reads new config fields
- The config schema and Rust enum have been updated if the prompt
  introduced a new `[[project.workflow]]` name
- TUI wiring is in place and verified via tmux E2E if TUI-launched
- If this is a new investigation category, `docs/vision.md` has been
  updated
