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

Review the target described in the assigned hub task: read the relevant
code, PRs, or docs; surface findings; open a PR with a fix if one is
clearly warranted; then report completion.

## Workflow

### 1. Read task context

Your task details are provided as the opening user message. The task ID,
description, links, and exact completion commands are all there. Re-read
them at any point with `hub task get <TASK-ID>`.

### 2. Understand the target

Using `Read`, `rg`, `fd`, and `gh`, explore what the task asks you to
review:

- Read the relevant code, PR diffs, issue threads, or documents.
- Understand the intent and the current behaviour.
- Check whether the concern raised in the task still applies.

If the concern is already resolved, call `hub task comment` explaining
what you found, then follow the completion steps from your task context.

### 3. Plan

Identify the review dimensions relevant to the target: correctness,
security, performance, test coverage, architecture, etc. Invoke
`/uphold-invariants` and use its constraints as a review lens.

If completing the review requires a design decision or context you cannot
determine from the code and history, call `hub task comment` with the
specific question, then `hub task report --status blocked`.

### 4. Review

Work through each dimension. For each finding, record:

- What the problem is, in domain terms.
- Where in the code it appears (file and line reference).
- Severity: blocking (must fix before ship) or advisory (worth tracking).

### 5. Fix blocking issues

For each blocking finding:

- Make the minimum change that addresses it in this worktree.
- Invoke `/uphold-invariants` to confirm the fix upholds all invariants.
- Add a test if the behavior can be verified without the full system.

If a fix would require a significant design decision not resolved by the
task description, call `hub task comment` with the question and stop.

### 6. Verify

If any code was changed:

```bash
just check && just test
```

Fix any failures introduced by your changes. Verify the fix manually
where possible.

### 7. Commit, push, and open a PR (if code changed)

If you made code changes:

Configure the repo author:

```bash
git config user.name "Michael Uloth"
git config user.email "hello@michaeluloth.com"
```

Invoke `/commit`, then invoke `/write-pr-description` to open a PR. The
PR body must reference the task (e.g. `Task: TASK-0042`). Skip any steps
requiring interactive input.

### 8. Write findings report

Write a clear findings section in your session log (see completion steps
below): what you reviewed, what you found, which issues were fixed in a
PR, and which advisories should be tracked as follow-up issues.

### 9. Complete the task

Follow the completion steps from your task context exactly.
