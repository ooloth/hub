## Purpose

Implements a GitHub issue autonomously: validates the issue's claims against current code, makes the fix in a prepared git worktree, verifies the repo's checks and tests pass, then opens a draft PR.

## Prerequisites

- `gh` CLI authenticated (`gh auth status`)
- `just` installed in the target repo
- Worktree already created and branch already checked out by the caller

## Context

The task prompt provides:

- `repo` — org/repo slug (e.g. `ooloth/hub`)
- `issue` — GitHub issue number
- `worktree` — absolute path to the prepared git worktree
- `branch` — branch name

## Workflow

### 1. Fetch the issue

```bash
gh issue view <issue> --repo <repo> \
  --json number,title,body,labels,comments,createdAt,updatedAt,state,author
```

Read the title, body, and all comments in full.

### 2. Verify the worktree is clean

```bash
git -C <worktree> status --porcelain
```

If the output is non-empty, the worktree has uncommitted changes — something went wrong in a previous run. Stop without modifying any labels.

### 3. Establish a baseline

Run the repo's check and test commands before making any changes:

```bash
cd <worktree> && just check && just test
```

If they fail, the repo was already broken before you touched it. Stop without modifying any labels — this is not a problem you introduced and not yours to fix.

### 4. Claim

Relabel immediately to prevent a second agent from claiming the same issue:

```bash
# Create label if absent
gh label list --repo <repo> --json name --jq '.[].name' | grep -q "status:agent-working" \
  || gh label create "status:agent-working" --repo <repo> \
       --color "0075ca" --description "An agent is currently implementing this"

gh issue edit <issue> --repo <repo> \
  --remove-label "status:ready-for-agent" \
  --add-label "status:agent-working"
```

### 5. Validate the issue's claims

Using `Read`, `rg`, and `fd` inside `<worktree>`, explore the areas the issue describes:

- Find the files and symbols it references
- Check whether the bug, gap, or violation it describes still exists in the current code
- Check recent commits for evidence it was already addressed:

```bash
git -C <worktree> log --oneline -20
```

**If stale or already resolved** — comment explaining what you found, relabel, and stop:

```bash
gh issue comment <issue> --repo <repo> --body "..."

gh label list --repo <repo> --json name --jq '.[].name' | grep -q "status:needs-human-review" \
  || gh label create "status:needs-human-review" --repo <repo> \
       --color "d93f0b" --description "Needs a human to review before proceeding"

gh issue edit <issue> --repo <repo> \
  --remove-label "status:agent-working" \
  --add-label "status:needs-human-review"
```

### 6. Plan

Read the relevant files and understand the module structure. Identify exactly which files need to change and why. If the fix requires a design decision not already resolved by the issue body and comments, comment on the issue with the open question, relabel to `status:needs-human-review`, and stop — do not guess.

### 7. Implement

Make all changes inside `<worktree>`. Follow the repo's existing conventions (formatting, naming, error handling, style). Do not touch files unrelated to the issue.

### 8. Fix until green

Run the repo's check and test commands:

```bash
cd <worktree> && just check && just test
```

Your changes introduced any failures that appear now — the baseline passed in step 3. Read the errors, fix them in `<worktree>`, and re-run. Repeat until green. Do not bail here: this is your responsibility to resolve.

If after multiple fix attempts the failures are intractable (e.g. the issue itself has a flaw that makes a correct implementation impossible), comment on the issue explaining the problem, relabel to `status:needs-human-review`, and stop.

### 9. Commit and push

Invoke `/commit` to stage, commit, and push your changes.

### 10. Open draft PR

Invoke `/write-pr-description` to draft and open the PR. The PR must be
a draft and the body must include `Closes #<issue>`.

### 11. Comment on the issue

```bash
gh issue comment <issue> --repo <repo> \
  --body "Opened draft PR: <pr-url>"
```

## Output format

**Done:** PR #N opened as draft — `<pr title>`

Or if stopped early:

**Stopped:** <one sentence reason> — issue relabeled `status:needs-human-review`
