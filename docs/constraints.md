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
- **Hub's GitHub token has only the `repo` scope, and one field that needs another scope fails the
  whole query.** On 2026-10-28 every pull request search failed because the query asked for a
  requested team's `slug`, which needs `read:org` or `read:discussion`; GitHub answered with
  `errors` and no data. A query checked through the `gh` CLI does not show this, since that token
  has broader scopes: check a new field against hub's own token, or against GitHub's documented
  scope for it, before adding it.
- **GraphQL does not say when a commit was pushed.** `Commit.pushedDate` returns null (read
  2026-10-19 on ooloth/hub#53), and a commit's `committedDate` is set when it is made, not when it
  is pushed. The REST repository activity API,
  `GET /repos/{owner}/{repo}/activity?ref=refs/heads/{branch}`, records each push, force push and
  branch creation with its time and the commit it left, newest first. It reached back to the
  creation of ooloth/hub (2026-04-16) when read on 2026-10-19. Once a branch is deleted, its newest
  activity leaves the all-zero commit. Nothing in hub reads it since #370.

## terminal-notifier

Measured on 2026-10-28 with terminal-notifier 3.1.0 on macOS, by posting test cards in a throwaway
group and reading the screen by eye and the group with `terminal-notifier -list`.

- **Posting into a group whose card is showing presents the card again.** A plain post, without
  `-remove` first, made the old card disappear and the new one appear, the same as removing and
  then posting. An expanded card is the exception: it updates in place without appearing again, and
  shows the new content once collapsed. So a card can be re-presented on every refresh with one
  call.
- **A collapsed card shows 3 message rows of about 37 characters; an expanded one about 10.** Longer
  lines wrap, and the rest is cut off with an ellipsis. Anything that must be read without
  expanding the card fits in the title, the subtitle and three short rows.
- **A card's text has no formatting.** Bold digits are possible only as Unicode's mathematical bold
  characters, which come from a different typeface than the system font and are read by VoiceOver as
  math symbols.
- **`-list` run right after a post can miss the card.** Once, a `-list` immediately after a post
  printed nothing, and the same call 2 seconds later listed the card. Seen a single time, so the
  delay is not known; nothing should read `-list` right after posting to confirm the post.
