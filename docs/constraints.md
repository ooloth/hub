# Constraints

Facts about the systems hub depends on that limit which designs work. Each says how it was
established, so a reader can tell how far to trust it and when to check it again.

## GitHub

- **A review names the commit its reviewer was looking at, not the head when it was submitted.**
  So whether a review came after a push could only be decided by comparing times, never by
  comparing the review's commit with the head. Hub no longer asks that question: whose move it is
  is read from a pull request's state in one refresh
  ([decision](decisions/030-whether-a-pull-request-is-my-turn-is-read-from-its-state-in-one-refresh.md)).
  Measured 2026-10-19 on the 40 most recently updated merged
  pull requests in astral-sh/ruff with more than two reviews: 198 of 547 reviews named a commit
  other than the branch head at the moment they were submitted, with the head at each moment read
  from the repository activity API. Some were submitted days after the newer push, as on
  astral-sh/ruff#26508.
- **GraphQL does not say when a commit was pushed.** `Commit.pushedDate` returns null (read
  2026-10-19 on ooloth/hub#53), and a commit's `committedDate` is set when it is made, not when it
  is pushed. The REST repository activity API,
  `GET /repos/{owner}/{repo}/activity?ref=refs/heads/{branch}`, records each push, force push and
  branch creation with its time and the commit it left, newest first. It reached back to the
  creation of ooloth/hub (2026-04-16) when read on 2026-10-19. Once a branch is deleted, its newest
  activity leaves the all-zero commit. Nothing in hub reads it since #370.
