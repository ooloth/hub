# 009 — Hub drops scheduled runs; Tier 3 Execute is human-triggered

> **⚠ Partially reversed by [Decision 012](012-task-model.md), then reversed
> again by [Decision 019](019-drop-task-model-filesystem-sessions.md).** 012
> deferred scheduled runs rather than abandoning them, routing their return
> through recurring `AgentTask`s. 019 dropped the task model, so that route is
> gone. The billing concern that motivated this decision remains valid. The
> Tier 1/2/3 framing below is replaced by the delegation flywheel described in
> [vision.md](../vision.md).

> **⚠ Amended by [Decision 020](020-hub-runs-an-unattended-surface.md).** Hub
> runs an unattended surface. Neither reason given below reaches it: it makes
> no `claude -p` calls, so it incurs none of the per-call cost that concern is
> about, and it needs the local `hub.toml`, the 1Password CLI and the local
> cache, which is the access this record names as the line Claude Desktop
> Routines cannot cross. What stays true is that agents are human-initiated:
> the unattended surface detects and notifies, and a keypress still starts
> every investigation.

## Context

Decision 006 described two launch modes for hub's investigation prompts:
TUI keypress (interactive) and scheduled `claude -p` run (unattended).
Decision 005 planned an `agents/` crate for background LLM automation.
The implied model was that hub would eventually run unattended — fetching
signals, scoring urgency, implementing labelled issues — without a human
initiating each action.

Two things changed:

1. **Billing model shift.** `claude -p` usage will no longer be covered
   by the Anthropic subscription. Unattended runs — which can invoke many
   Claude calls in sequence — would accrue per-call costs with no human
   deciding each invocation is worth it.

2. **Claude Desktop Routines fills the gap.** Lightweight scheduled
   investigations (reminders, digests, cross-project summaries) that
   *don't* require local system access run well as Claude Desktop
   Routines, which are covered by the subscription. Hub doesn't need to
   own that tier.

The one thing Claude Desktop Routines *can't* replicate is what
`implement-issue` does: create a git worktree, configure credentials,
run `gh` CLI commands, stream structured output, write transcripts. That
capability is worth keeping — but it doesn't require an unattended
trigger. A human keypress is a better trigger anyway: it keeps the
human informed and in control at the moment an agent is about to write
code.

## Decision

Hub drops all scheduled and unattended runs. There is no `claude -p`
invocation path, no cron trigger, and no planned `agents/` crate.

All three tiers of hub's observe→understand→act loop are
**human-initiated**:

| Tier | What hub does | Trigger |
|---|---|---|
| 1 — Surface | Ranked attention dashboard | TUI opens |
| 2 — Investigate | Multi-turn Claude investigation | Keypress (`i`) on a TUI item |
| 3 — Execute | Autonomous implementation in a worktree | Keypress on a ready issue |

Tier 3 Execute — currently `implement-issue`, accessible as a CLI
command — migrates to a TUI action. The user selects an issue, presses a
key, and hub launches the implementation session in a tmux split, the
same way Tier 2 investigations work today. The worktree, credentials,
and transcript machinery remain; the scheduling and CLI entry point are
removed.

The `agents/` crate described in Decision 005 will not be built. The
use cases it was intended for either don't exist yet or are covered by
the TUI + prompts model.

## Consequences

- `prompts/` is a TUI-launch-only library. The "automation
  compatibility" constraint — never ask the user a question — is lifted.
  Prompts can and should invite human input when it's warranted.
- `workflows/src/implement.rs` loses its CLI entry point and gains a TUI
  action module in `ui/tui/src/investigations/`.
- `prompts/implement-issue.md` stays; `prompts/repo-scan.md` is removed
  (it had no TUI trigger and no planned replacement).
- Decision 005's `agents/` section is superseded by this decision.
- Decision 006's capability table is updated: "scheduled run" is no
  longer a launch mode.
- Claude Desktop Routines handles any lightweight scheduling needs until
  recurring tasks are implemented. Hub does not compete with or duplicate that.
