---
number: 029
status: accepted
date: 2026-10-26
---

# 029 — Hub opens its windows in whichever of tmux or Rex encloses it

## Forced by

- I use [Rex](https://www.superlogical.com/rex/docs) as my terminal, and Rex is meant to be a
  native terminal multiplexer that removes the need for tmux. Hub's investigations, lazygit, Octo
  and the PR diff opened only in tmux windows, so in Rex they needed tmux running inside a Rex pane.
- [Decision 015](015-accept-tmux-and-claude-code-coupling.md) accepted the tmux coupling with no
  abstraction "until there is a second concrete runner to abstract over". Rex is a second concrete
  terminal.
- Observations on 2026-10-26, in a throwaway Rex session:
  - **Rex has no option for a terminal's environment.** The terminal block's options are `shell`,
    `command`, `args`, `login`, `cwd`, `path_prepend` and `size` (`rex block types --json`).
    *Measured.*
  - **Arguments reach the program unchanged.** A 5.4 KB value of quotes, `$`, backticks, `$(…)`,
    backslashes, `\r\n` and multibyte text came through `rex window new … -- env K=V /bin/sh -c …`
    byte for byte, with and without `--shell=none`. *Measured.*
  - **Rex refuses a request over 64 KB** (`control message too large … limit is 65536`), and tmux
    3.8 refuses a `-e` value of 17 KB (`command too long`) while accepting 10 KB. *Measured.* Rex's
    limit is the looser of the two.
  - **A Rex tab closes when its command exits**, as a tmux window does, and `rex window focus <id>`
    makes the window the session's active one. *Measured.*

## Decision

Hub opens each window in the terminal that encloses the TUI. When `TMUX` is set it uses tmux, as
before. When `TMUX` is absent and `REX_SESSION` is set, it opens a Rex tab in that session. With
neither, the action fails with a message naming both. The TUI itself starts and shows signals
either way; only the window actions need a terminal.

tmux decides first because Rex is a GUI terminal and so always the outermost layer. tmux started in
a Rex pane inherits `REX_SESSION`, but the windows the user can see there are tmux's.

The two terminals are one `Terminal` enum in `ui/tui/src/terminal/`, matched in each operation, with
each terminal's invocations in a module of its own. Rex tabs receive hub's environment as
`/usr/bin/env K=V … /bin/sh -c COMMAND`, each value its own argv word, which keeps the guarantee
`tmux -e` gives: no prompt text passes through a shell parser. The command runs under `/bin/sh`
rather than the user's shell, because hub writes every command it runs and a fixed shell gives each
one the same meaning on every machine.

This supersedes Decision 015's tmux section. Its Claude Code section is unchanged.

## Rejected

- **Run tmux inside Rex.** Its strongest case: it needs no change to hub, and it already works.
  Rejected because it keeps the layer Rex exists to remove, and hub's windows would never be Rex
  tabs. Reverses if Rex stops being the terminal I use.
- **A trait with a tmux and a Rex implementation.** Its strongest case: a third terminal would add
  an implementation without touching the others. Rejected because the set is closed and both live
  in one module, so a `match` shows every terminal's behaviour for one operation in one place, where
  a trait sends the reader through dynamic dispatch to an implementation nothing outside the module
  could provide. Reverses if a third terminal arrives, or terminals start differing in which
  operations they support.
- **A `hub.toml` setting choosing the terminal.** Its strongest case: no guessing from the
  environment. Rejected because the environment already says which terminal encloses the TUI, so a
  setting adds configuration that can only disagree with it. Reverses if a case appears where the
  environment points at the wrong terminal.
- **Detecting the terminal once at startup.** Its strongest case: one detection per run. Rejected
  because a TUI started outside both would either refuse to start or carry an `Option` through
  every launch, only to fail where the window action fails today.

## Risk

If a Rex server is started from inside tmux, its terminals may inherit `TMUX`, and hub would then
open tmux windows that are not on screen. Hub cannot tell that case apart from tmux running inside
Rex. The Rex app normally starts its own server, so this has not been seen.

## Revisit when

A third terminal is wanted, Rex gains an option for a terminal's environment (the `env` wrapper can
then go), or the case under Risk is seen.

## Also update

- [x] Decision 015 — marked as superseded for its tmux section.
- [x] vision.md, README.md, ui/tui/README.md and CLAUDE.md — windows are tmux windows or Rex tabs.
- [ ] The TUI QA loop in CLAUDE.md still drives the TUI in tmux. Moving it to Rex is a separate
      change.
