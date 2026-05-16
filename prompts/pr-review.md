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

The initial message tells you:
- PR number and repo
- PR author (for ToReview PRs) or confirmation this is your own PR
- Which skill was selected and the intent

## Prerequisites

- `gh` CLI authenticated (`gh auth status`)
- The PR branch is already checked out in the current worktree
- Run `git log --oneline -5` to confirm you are on the right commit
