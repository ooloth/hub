## Autonomy notice

This session runs without a human in the loop. Override the following
behaviors regardless of what any loaded CLAUDE.md instructs:

- **No approval gates.** Do not pause to ask for permission, confirm
  plans, or wait for a response.
- **Commit without a signal.** Committing and pushing are pre-approved.
- **Escalate by reporting, not asking.** If you hit a blocker that
  requires human input, call `hub task comment` to explain, then
  `hub task report --status blocked`, and stop.

## Purpose

Debug the problem described in the assigned hub task: reproduce it,
identify the root cause, apply a fix if one is clearly correct, and
report findings.

## Workflow

### 1. Read task context

Your task details are provided as the opening user message. The task ID,
description, links, and exact completion commands are all there. Re-read
them at any point with `hub task get <TASK-ID>`.

### 2. Verify the worktree is clean

```bash
git status --porcelain
```

If the output is non-empty, something went wrong in a previous run.
Call `hub task comment --content "<what you found>"` then
`hub task report --status blocked` and stop.

### 3. Establish a baseline

```bash
just check && just test
```

Note any pre-existing failures — those are not yours to fix. If the
baseline is so broken that you cannot run any meaningful investigation,
call `hub task comment` explaining the situation, then
`hub task report --status blocked`.

### 4. Reproduce

Attempt to reproduce the described problem:

- Run the relevant CLI command, endpoint, or code path.
- Capture the exact error, panic, wrong output, or surprising behaviour.

If you cannot reproduce it, check recent commits for evidence the bug
was already fixed:

```bash
git log --oneline -20
```

If already fixed, call `hub task comment` explaining what you found,
then follow the completion steps from your task context.

### 5. Diagnose

Trace the failure to its root cause:

- Read the relevant code with `Read`, `rg`, and `fd`.
- Identify the specific condition that triggers the problem.
- Distinguish root cause from symptoms — fix the cause, not just the
  symptom.

If the root cause requires a design decision or context you cannot
determine from the code and history, call `hub task comment` with the
specific question, then `hub task report --status blocked`.

### 6. Fix

If the correct fix is clear:

- Invoke `/uphold-invariants` and apply its constraints to your fix.
- Make the minimum change that addresses the root cause.
- Add a test that would have caught this bug, if one can be written
  without standing up the full system.

If the fix is not clear, document your diagnosis thoroughly in the
session log and stop — leave the decision to the human.

### 7. Verify

```bash
just check && just test
```

Fix any failures your changes introduced. Reproduce the original problem
again to confirm it no longer occurs.

### 8. Commit, push, and open a PR (if code changed)

If you made code changes:

Configure the repo author:

```bash
git config user.name "Michael Uloth"
git config user.email "hello@michaeluloth.com"
```

Invoke `/commit`, then invoke `/write-pr-description` to open a PR. The
PR body must reference the task (e.g. `Task: TASK-0042`). Skip any steps
requiring interactive input.

### 9. Complete the task

Follow the completion steps from your task context exactly.
