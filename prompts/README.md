# prompts/

System prompts for hub's two agent session types.

## investigations/

Loaded by `ui/tui/src/investigations/*.rs` via `include_str!` when a human
presses `i` on a signal item. These sessions are interactive — the human is
watching — and run with a restricted tool set (`--allowedTools`).

Each file covers one signal kind: `ci.md`, `gcp.md`, `issue.md`, `loki.md`,
`media.md`.

## These prompts are the tuning surface

Improving these prompts is how agent quality compounds — they are the lever the
flywheel turns (see [../docs/vision.md](../docs/vision.md)). The planned mining
loop ([Decision 018](../docs/decisions/018-meta-loop-output-as-labeled-issues.md))
reads agent session logs and the delta between agent output and what shipped,
then proposes edits to these files.
