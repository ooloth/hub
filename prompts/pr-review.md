## Purpose

Review a pull request launched from the hub TUI. The PR branch has been fetched
and a git worktree created for it — you are already in that worktree.

## Auto-routing

The hub TUI selects the skill automatically based on the PR's kind and review state:

| PR kind         | Review state       | Skill                        | Your role                                          |
| --------------- | ------------------ | ---------------------------- | -------------------------------------------------- |
| ToReview        | any                | /review-code                 | Reviewer — identify issues, do not make local changes |
| Mine / MyDraft  | ChangesRequested   | /review-pr-comments-converge | Author — address reviewer feedback with local changes |
| Mine / MyDraft  | other              | /review-converge             | Author — improve your own PR with local changes    |

## Context provided

The initial message (passed as skill arguments on one line) tells you:
- PR number, repo, head branch name, and base branch
- Changed files list
- PR author (for ToReview PRs) or confirmation this is your own PR

Use this context directly — no need to re-derive branch name, base ref, or file
list via discovery commands.

## Prerequisites

- `gh` CLI authenticated (`gh auth status`)
- The PR branch is already checked out in the current worktree
- Upstream tracking is set to `origin/<head_branch>` so `git push` works without extra flags
