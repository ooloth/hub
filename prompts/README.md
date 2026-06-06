# prompts/

System prompts for hub's two agent session types.

## investigations/

Loaded by `ui/tui/src/investigations/*.rs` via `include_str!` when a human
presses `i` on a signal item. These sessions are interactive — the human is
watching — and run with a restricted tool set (`--allowedTools`).

Each file covers one signal kind: `ci.md`, `gcp.md`, `issue.md`, `loki.md`,
`media.md`.

## tasks/

Loaded by `workflows/src/dispatch.rs` via `include_str!` when the 30-second
dispatch tick claims a ready task. These sessions are autonomous — no human
is present — and run with full permissions
(`--dangerously-skip-permissions`).

Each file covers one task kind: `implement.md`, `review.md`, `debug.md`.
The task-specific context (title, description, links, comments, completion
steps) is built at dispatch time and passed separately as `HUB_TASK_PROMPT`.
