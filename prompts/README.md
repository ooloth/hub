# prompts/

System prompts for hub's agent sessions. Only investigation prompts exist, and `scheduled/` is
empty.

## investigations/

Loaded by `ui/tui/src/investigations/*.rs` via `include_str!` when a human
presses `i` on a signal item. These sessions are interactive — the human is
watching — and pre-approve the tools each kind needs (`--allowedTools`).

Each file covers one signal kind: `ci.md`, `gcp.md`, `issue.md`, `loki.md`,
`media.md`.

`investigations/media.md` is a symlink into hub-private
(`prompts/media-investigate.md`), so it does not resolve without hub-private checked out beside hub.

## These prompts are the tuning surface

Improving these prompts is how agent quality compounds — they are the lever the
flywheel turns (see [../docs/vision.md](../docs/vision.md)). A loop that reads
agent session files and the delta between agent output and what shipped, then
proposes edits to these files, is kept as an idea with no design yet:
[Decision 018](../docs/decisions/018-meta-loop-output-as-labeled-issues.md) is
superseded by [Decision 019](../docs/decisions/019-drop-task-model-filesystem-sessions.md).
