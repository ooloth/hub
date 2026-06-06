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

Implement the hub task assigned to this session: validate the stated
problem against the current code, make the fix in this worktree, verify
all checks and tests pass, then report completion.

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

Run the repo's check and test commands before touching anything:

```bash
just check && just test
```

If they fail, the repo was already broken before you arrived. Call
`hub task comment` explaining the baseline failures, then
`hub task report --status blocked`.

### 4. Validate the task's claims

Using `Read`, `rg`, and `fd`, explore the areas the task describes:

- Find the files and symbols it references.
- Confirm the problem it describes still exists in the current code.
- Check recent commits for evidence it was already addressed.

If already resolved, call `hub task comment` explaining what you found,
then follow the completion steps from your task context.

### 5. Plan

Read the relevant files and understand the module structure. Invoke
`/uphold-invariants` and apply its constraints to every decision.

Identify exactly which files need to change and how. If the fix requires
a design decision not resolved by the task description or comments, call
`hub task comment` with the open question, then
`hub task report --status blocked`.

### 6. Implement

Make all changes in this worktree. Follow the repo's existing
conventions (formatting, naming, error handling, style). Do not touch
files unrelated to the task.

### 7. Write missing tests

Before running checks, ask: what new decisions or behaviors did this
change introduce? For each: if the logic were wrong, would any existing
test catch it? If not, and if the behavior can be exercised without
standing up the full system, write a test.

### 8. Fix until green

```bash
just check && just test
```

Your changes introduced any failures that appear now — the baseline
passed in step 3. Read the errors, fix them, and re-run. Repeat until
green. If failures are intractable after multiple rounds, call
`hub task comment` explaining the problem, then
`hub task report --status blocked`.

### 9. Verify manually

Ask: how can I run this and confirm it actually works? Run the CLI, hit
the endpoint, trigger the event, eyeball the output — whatever applies.
Do not rely on tests alone. If end-to-end execution is impossible, say
why explicitly.

### 10. Self-review

Read the full diff and steelman a reviewer's objections:

- Do these changes solve the problem? Does the test suite prove it?
- Are edge cases or unwanted side effects missed?
- Is this the minimum change, or did anything speculative creep in?
- Do these choices uphold all relevant invariants?

Fix anything you can't defend.

### 11. Commit and push

Configure the repo author before committing:

```bash
git config user.name "Michael Uloth"
git config user.email "hello@michaeluloth.com"
```

Invoke `/commit` to stage, commit, and push. Committing is pre-approved
for this session — skip any steps that require a user signal.

### 12. Open a PR

Invoke `/write-pr-description` to draft and open a PR. The PR body must
reference the task (e.g. `Task: TASK-0042`). Skip any steps requiring
interactive input — omit those fields rather than using placeholders.

### 13. Complete the task

Follow the completion steps from your task context exactly.
