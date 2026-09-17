---
opened: 2026-09-16
status: open
resolves_into: decision
---

# Should `[[project]].name` be a short display label, separate from the clone directory?

## Why it matters

`[[project]].name` does two jobs that pull in opposite directions. `config/src/toml.rs:33-34`
documents it as "Human-readable project name shown in the TUI", and `launch.rs`'s `project_name()`
feeds it to `workflows::fetch::repos_dir().join(name)`, making it the directory holding the bare
clone. So shortening the label to suit a human strands an existing clone, and nothing in the field's
name warns you.

Window names made this visible. A tmux status bar showing several investigations at once spends its
width on the project segment: `michaeluloth-com:pr:12` is 22 characters where `mu:pr:12` is 8. With
five windows open that is the difference between a bar you can read and one tmux elides.

There is also an inconsistency already in place. Repo-derived windows take their project segment
from the stripped repository slug, while alert windows take it from `project`, which comes from
config. So two windows for the same project can disagree about what that project is called, and
nothing currently forces them to match.

None of this is broken. Widening the segment budget (commit `cf65673`) means no repository in use
today is shortened at all. What remains is legibility and a field whose meaning is doing too much.

## What would settle it

Routinely having enough investigation windows open that tmux elides their names. Until that
happens the gain is theoretical and the cost of a config change is not.

A second trigger would be the repo-versus-alert inconsistency actually misleading someone about
which project a window belongs to.

What would *not* settle it is the intuition that shorter is nicer, which is what every option here
already assumes.

## Resolves into

Depends on which option wins. Adding an optional field is additive and belongs in the issue that
implements it. Renaming `name`, or changing what it controls, breaks every device's `hub.toml` and
the JSON schema that validates it, which moves a boundary and belongs in `../decisions/`.

## Source

Raised while building #330, which introduced `domain::InvestigationWindow` and made the project
segment visible in every tmux window hub opens.

## Options

- **A. Nothing.** After the budget widening no real name is shortened, and a window name that
  depends on config also stops being a pure function of the signal, which is the property #331
  relies on to find a window by rebuilding its name. Changing a label would orphan windows opened
  under the old one.
- **B. Add an optional `short`, defaulting to `name`.** Additive, no migration, one job per field.
  Costs an entry in `config/schemas/hub.toml.schema.json`, the taplo validation riding on it, an
  `add-a-project` playbook update, and a third name per project to keep straight.
- **C. Decouple the clone directory from the label, then rename the field.** Derive the clone
  directory from the repository slug, freeing `name` to be purely a display label, renamed to
  something that says so. One name per project instead of two, and the field's meaning matches its
  use. Costs a migration for existing clones under `~/.hub/repos/` and a breaking config change
  across every device.
- **D. Use `name` for repo windows too.** Fixes the repo-versus-alert inconsistency on its own,
  without any new field or rename, and lets a shorter `name` shorten windows today at the existing
  cost of renaming the clone directory. Smallest change that improves anything.

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it graduates
into a decision record._

- *Sourced:* `config/src/toml.rs:33-34` documents `name` as the TUI display name; `launch.rs`'s
  `project_name()` and `workflows::fetch::repos_dir()` make it the clone directory. The double duty
  is not recorded anywhere as deliberate.
- *Sourced:* `domain/src/loki.rs:45-46` documents `LokiEntry.project` as "Hub project name this
  entry belongs to", which is what alert windows use, unlike repo windows.
- *Measured:* with the project budget at 20 characters, every repository currently configured
  survives intact. `domain/src/investigation_window.rs` pins this in
  `real_repository_names_survive_intact`, so a future budget change that would start shortening
  them fails rather than passing quietly.
- *Reasoned:* deriving any part of a window name from config weakens the determinism #331 depends
  on. The failure is mild, a duplicate window after a deliberate config edit, rather than silent
  wrongness, but it is the same category as the sequence-suffix scheme rejected during #330.
