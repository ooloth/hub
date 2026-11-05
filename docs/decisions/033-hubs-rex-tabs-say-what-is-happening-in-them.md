---
number: 033
status: accepted
date: 2026-11-04
---

# 033 — hub's Rex tabs say what is happening in them

## Forced by

- `monitor-prs` recognises a review it or anyone else prepared only by a tab labelled
  `PR <number> (<author login>): <skill>`, and counts every such `review-*` tab toward its limit
  ([dotfiles 010](https://github.com/ooloth/dotfiles/blob/main/docs/decisions/010-an-open-tab-is-the-only-record-of-what-monitor-prs-has-prepared.md)).
  A tmux window name, such as `hub:pr:330`, is not a label `monitor-prs` can see.
  [hub#394](https://github.com/ooloth/hub/issues/394).
- The dotfiles launcher names every Claude tab `<subject> (<person>): <activity>`, and hub's tabs
  share its sessions ([032](032-hubs-rex-tabs-open-in-their-repositorys-session.md)).
- A PR's investigation and every review share one window name, so asking for a second one reuses
  the first tab and its prompt never runs.
- hub finds an open tab by building its name again
  ([019](019-drop-task-model-filesystem-sessions.md)), so a label must come from the signal alone
  and be the same every time.
- hub keeps a tmux window name to at most 40 characters with no whitespace, which these labels
  can't meet.

## Decision

Each Rex tab hub opens is labelled `<subject> (<author login>): <activity>`, and hub looks for an
open tab by that label in the tab's session.

| Tab | Label |
|---|---|
| `i` on a PR | `PR 12 (alice): investigate` |
| A review | `PR 12 (alice): review-code`, `review-converge` or `review-pr-comments-converge` |
| lazygit, Octo, the diff | `PR 12 (alice): lazygit`, `octo` or `diff` |
| An issue | `Issue 412 (bob): investigate` |
| CI | `CI test-suite: investigate` |
| A Loki or GCP alert | `Loki prod <excerpt>: investigate` |
| A media item | `Media <title>: investigate` |

The author is the GitHub login of whoever opened the PR or issue, and a subject nobody wrote has no
parentheses. The activity is the skill the tab runs, or one plain word, so each review skill has
its own tab. A tab the launcher opened with the same label is the same tab, and hub reuses it.

Text that comes from a signal has its line breaks and control characters removed and is cut to a
fixed length. An alert's environment is also lowercased, with each run of other characters made
one hyphen. An alert's excerpt is the start of its message. When a piece of a
signal's text, such as an alert's message, a CI workflow's name or a media item's title, is not
shown whole, it ends in `…` and a six-character hash, so two signals of one repository never share
a label.

tmux window names don't change, and in tmux a PR's investigation and reviews still share one
window.

## Rejected

- **Deriving the label from the tmux name.** Its strongest case: one name per window. Rejected
  because the tmux name has no author, no skill and no readable message. Reverses if tmux names
  take this form too.
- **`Alert prod: <excerpt>`, with the excerpt in the activity's place.** Its strongest case: a
  shorter label. Rejected because it breaks the form every other tab uses. Reverses if alert
  labels prove too long to read in Rex's tab bar.
- **Also finding tabs by their old tmux-style name.** Its strongest case: a tab opened before this
  change is still found. Rejected because it adds a lookup to every launch for tabs that close by
  themselves, and the duplicate it prevents happens once. Reverses if old tabs turn out to stay
  open for weeks.

## Risk

A tab someone relabels by hand in Rex is no longer found, and asking for it opens a second one. So
is a tab whose author renames their GitHub account.

hub looks for a tab only in its repository's session. A tab with the same label in any other
session, such as one the launcher opened there with `-s` or one moved to another session, is not
found, and asking for it opens a second one.

A label doesn't name the repository, and a session's label names only the repository, not its
owner. So a tab for PR 12 of another owner's repository with the same name, by the same author and
with the same activity, is taken for hub's own and reused. hub refuses two tracked projects whose
repositories share a name, and launches investigations and reviews only on tracked projects, so
only a tab from outside them can be taken this way: one the launcher opened, or a diff hub opened
for a repository no project tracks.

In Rex, a PR's investigation and each review get their own tab, but all of them work in the PR's
one `pr-<N>` worktree. A review that changes files, such as `review-converge`, runs beside the
others there, and nothing stops two of them changing it at once.

## Revisit when

`monitor-prs` or the launcher change their label form, or Rex lets a tab carry an identifier hub
could find it by instead of its label.

## Also update

- [x] dotfiles `monitor-prs`: put back "and hub opens them in each repository's".
- [x] CLAUDE.md's Rex QA loop: the tab it lists is labelled `Issue <n> (<login>): investigate`.
