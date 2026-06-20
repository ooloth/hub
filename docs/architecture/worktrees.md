# Two worktree systems

> **⚠ Partially superseded by [Decision 019](../decisions/019-drop-task-model-filesystem-sessions.md) (accepted; not yet implemented).**
> PR investigation worktrees are unchanged. The "task dispatch worktrees" system
> below is removed — those worktrees become general `i`-launched investigation
> sessions with no task lifecycle. Accurate as built until 019 is implemented.

Hub uses git worktrees in two distinct contexts. They share the same underlying
`git worktree add` mechanism but exist at different paths, serve different
audiences, and have different lifetime and cleanup rules. Do not conflate them.

## Side-by-side comparison

| Dimension | PR investigation worktrees | Task dispatch worktrees |
|---|---|---|
| **Purpose** | Human-interactive sessions (review, fix, ask) | Agent task sessions (implement, debug, review) |
| **Location** | `~/.hub/repos/<project>/pr-<N>/` | `~/.hub/workspaces/TASK-XXXX/<project>/` |
| **Branch** | PR head branch (e.g. `fix/login`) | `agent/TASK-XXXX` (fresh from trunk) |
| **Lifetime** | Ephemeral — cleaned up when remote head deleted | Persistent — survives session termination |
| **Cleanup trigger** | Remote head branch deleted (merged/closed PR) | Task terminal ≥72h **and** no unpushed commits |
| **Owned by** | `workflows/src/fetch.rs` | `workflows/src/dispatch.rs` (planned, issues #278–#279) |
| **TUI entry point** | `WorktreeSpec` in `ui/tui/src/investigations/launch.rs` | Dispatch loop in TUI (not yet built) |

## PR investigation worktrees

Created on demand when a human opens a PR in lazygit or octo. The bare repo
at `~/.hub/repos/<project>/` is the parent; the worktree is a linked checkout
at `~/.hub/repos/<project>/pr-<N>/` on the PR's head branch.

Cleanup runs as part of `fetch::run()`: after each fetch, any worktree whose
remote tracking ref has disappeared (branch deleted on GitHub after merge or
close) is removed automatically. See `workflows/src/fetch.rs`.

## Task dispatch worktrees

Created by `ensure_task_worktree()` (planned — issue #278) when the dispatch
loop claims a ready task. The same bare repo under `~/.hub/repos/<project>/`
is the parent; the worktree is added at `~/.hub/workspaces/TASK-XXXX/<project>/`
on a fresh branch named `agent/TASK-XXXX`.

Task worktrees persist across session boundaries — the agent resumes in the
same worktree with prior commits intact. Cleanup is deliberately deferred:
a worktree is only removed when ALL three conditions hold simultaneously:

1. The task is in a terminal state (`done`, `failed`, `cancelled`)
2. It has been terminal for ≥72 hours
3. The worktree has no unpushed commits (`git log @{u}..HEAD` is empty)

Condition 3 is a hard safety invariant: work that has not reached the remote
is never destroyed automatically. See `docs/architecture/task-dispatch.md` for
the full dispatch design.
