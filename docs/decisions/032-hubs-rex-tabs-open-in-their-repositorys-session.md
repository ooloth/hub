---
number: 032
status: accepted
date: 2026-11-04
---

# 032 — hub's Rex tabs open in the background, in their repository's session

## Forced by

- The dotfiles launcher opens each repository's Claude tabs in a session labelled
  `Workers (<repo>)`
  ([dotfiles 011](https://github.com/ooloth/dotfiles/blob/main/docs/decisions/011-claude-tabs-open-in-their-repositorys-rex-session.md)),
  and `monitor-prs` reads every session's tabs to decide what it has prepared
  ([dotfiles 010](https://github.com/ooloth/dotfiles/blob/main/docs/decisions/010-an-open-tab-is-the-only-record-of-what-monitor-prs-has-prepared.md)).
  hub opens its tabs in the TUI's own session, apart from the launcher's tabs for the same
  repository. [hub#394](https://github.com/ooloth/hub/issues/394).
- hub finds an open tab by building its name again
  ([019](019-drop-task-model-filesystem-sessions.md)), so two repositories' tabs with the same name
  must not be looked up among each other.
- Rex.app changes the tab it shows only through `session.select` with Remote Control on, and a tab
  created without `--focus=false` becomes its session's active tab, which sends key-bound actions to
  a pane the app isn't showing (Rex feedback, 2026-10-27 and 2026-11-01).

## Decision

Each Rex tab hub opens goes in the session labelled `Workers (<repo>)`, where `<repo>` is the part
of the signal's repository slug after the `/`. A signal with no repository uses `Workers`. hub uses
the first session with that exact label, creates it with the tab as its first window when there is
none, and looks for an open tab only in that session. Tabs open with `--focus=false`, asking for a
tab that is already open leaves the view as it is, and the status bar says which session the tab is
in.

tmux windows are unchanged. `REX_SESSION` still decides that Rex encloses the TUI, as 029 says, but
no longer chooses the session.

This supersedes 029's rule that Rex tabs open in the TUI's session. The rest of 029 stands.

## Rejected

- **Tabs in the TUI's session.** Its strongest case: one place to look, and no session to find or
  create. Rejected because two repositories' tabs with the same name would be looked up among each
  other once labels stop naming the project, and hub's tabs would sit apart from the launcher's for
  the same repository. Reverses if Rex gains a way to group tabs within a session.
- **Naming the session from the worktree's `origin`, as the launcher does.** Its strongest case:
  one rule with one source. Rejected because the diff and media tabs have no checkout to ask, and
  for every repository hub clones from GitHub, the slug's name is the name `origin` ends in.
  Reverses if hub tracks a repository whose `origin` name differs from its GitHub name.
- **Bringing tabs into view through Remote Control.** Its strongest case: `i` would land me in the
  tab. Rejected because Remote Control lets any process that reaches the Rex server drive the app.
  Reverses if Rex lets a process move its own client's view without it.

## Risk

Two repositories with the same name under different owners share a session. If the launcher and
hub create a missing session at the same moment, two sessions share the label, and each uses the
first.

## Revisit when

hub tracks two repositories with the same name, or Rex lets a process bring a tab into view without
Remote Control.

## Also update

- [x] 029: marked as superseded for its session rule.
- [x] CLAUDE.md's Rex QA loop: investigations open in `Workers (hub)`, not the QA session.
