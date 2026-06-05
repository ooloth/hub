# 015 — Accept tmux and Claude Code as baked-in dispatch dependencies

## Context

During pre-implementation review of the task dispatch epic (#278–#282), a
lifecycle spike validated the completion detection model and surfaced two
concrete coupling questions:

1. Is the dispatch system too tightly coupled to **tmux** to swap in another
   process manager?
2. Is it too tightly coupled to **Claude Code** (the `claude` CLI, its
   `~/.claude/sessions/` internal files) to swap in another agent runner?

The question arose because a pricing change to Claude Code subscriptions could
motivate switching runners, and multi-agent workflows (using different models or
providers for different task kinds) may be desirable in future.

---

## Decision

Accept the coupling to both tmux and Claude Code **as-is**. Do not introduce
abstraction layers. Isolate each concern in its own module without abstracting
across it.

---

## Why

### Tmux

The tmux dependency lives in exactly two places in the dispatch path:

- `tmux new-window -d -n TASK-XXXX ...` — spawning the agent window
- `tmux kill-window -t TASK-XXXX` — window reaping after `in-review`

This is ~5 lines in `workflows/src/dispatch.rs`. Swapping tmux for any other
process manager (screen, a raw subprocess, a container runtime) is a one-hour
change. No abstraction is worth its maintenance cost at this scale. Pure YAGNI.

### Claude Code

The Claude Code coupling is layered, and each layer has a different swap cost:

**Process flags** (`claude --session-id ... --append-system-prompt ... "prompt"`):
These are ~10 lines in `dispatch.rs`. Changing runners means changing these
lines. Trivial. YAGNI.

**Session file detection** (`~/.claude/sessions/<pid>.json` `status`/`updatedAt`
polling): This is the deepest coupling. Crucially, it is already documented as
fragile (undocumented internal API) and it only supports the **fallback** path.
The primary signal — `hub task report TASK-XXXX --status in-review` — is
completely runner-agnostic: any agent that can execute a shell command can call
it. If the session file format changes or a second runner has no equivalent, the
fallback degrades gracefully (tasks stay `in-progress` until manual correction)
while the primary path remains intact.

**Prompt format**: kind-specific system prompt + task context message. Portable
to any LLM runner with a system-prompt mechanism. Not a coupling concern.

### Full abstraction cost vs. benefit

A `AgentRunner` trait or enum would require: a trait definition, dispatch
branching for every runner-specific behaviour (process flags, session detection,
resume command), a config option to select the runner. This adds real complexity
for a theoretical future need. The correct time to add that abstraction is when
there is a second concrete runner to abstract over — not before.

---

## Isolation without abstraction

Rather than abstracting, each runner-specific concern lives in its own module:

- `workflows/src/dispatch.rs` — process spawn and window reap (tmux + `claude`
  flags). The only file that needs to change if either dependency changes.
- `workflows/src/agent_session.rs` — Claude Code session file polling. The only
  file that needs to change if `~/.claude/sessions/` format changes.

This boundary is enforced by code organisation, not by a trait.

---

## Consequences

**If tmux is replaced**: change ~5 lines in `dispatch.rs`. No other file is
affected.

**If Claude Code pricing changes**: change the process flags in `dispatch.rs`.
Optionally simplify `agent_session.rs` to a no-op (run on primary-signal-only).

**If a second agent runner is needed** (Codex, Gemini CLI, etc.): add a dispatch
branch in `dispatch.rs` (`match task.kind.agent_runner() { ... }`); add a
corresponding detection strategy in `agent_session.rs` or a new peer module.
The primary signal (`hub task report`) already works for any runner that can
execute shell commands. The `TaskKind::model()` → `TaskKind::agent_config()`
evolution path is one rename away.

**If `~/.claude/sessions/` format changes**: update `agent_session.rs` only.
The primary signal and all other dispatch logic are unaffected. Degraded
fallback behaviour (manual correction required) is acceptable during the window
before the fix lands.

---

## Reversibility

Two-way door. Abstraction can be added at any time, and the isolation boundary
established by `dispatch.rs` + `agent_session.rs` makes the refactor mechanical:
extract the runner-specific code from each file into a trait implementation.

**Revisit trigger**: a pricing change that makes Claude Code unaffordable for
unattended sessions, or a concrete need to dispatch tasks to a second runner.
