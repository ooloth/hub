---
number: 034
status: accepted
date: 2026-11-18
---

# 034 — Investigations start through the launcher hub.toml names

## Forced by

- On the work laptop, a Claude session needs the Vertex AI project, the work logins and the
  related-repo hints that the dotfiles launcher, `cc.bash`, provides. hub started bare `claude`, so
  its sessions failed with "No projectId was given" whenever sessions moved to Vertex AI.
  [#434](https://github.com/ooloth/hub/issues/434).
- hub is public and runs on machines without dotfiles
  ([002](002-public-repo-private-integrations.md)), and per-machine settings live in `hub.toml`
  ([003](003-config-model.md)).
- Investigations run in auto mode because they read text other people wrote
  ([023](023-investigation-prompts-fence-foreign-text.md)), and whether they should run with
  permissions skipped is an open question hub owns.

## Decision

`launcher` under `[investigation]` in `hub.toml` names, by absolute path, an executable hub runs in
place of `claude` for every investigation and review. Absent, hub runs `claude` from `PATH`. hub
passes the launcher the arguments it would pass `claude`, its `--permission-mode` and
`--append-system-prompt` included, and a launcher passes them on to `claude`. The launcher's path
reaches the window through its environment, as the prompts do. The TUI refuses to start when the
path is not an executable file. The daemon, which launches nothing, checks only that the path is
absolute.

## Rejected

- **The dotfiles launcher's path written into hub's code:** hub is public and runs on machines
  without dotfiles.
- **hub's own copy of the launch logic:** it would track the work logins and private values on its
  own, and drift from the launcher.
- **Leaving the permission mode to the launcher:** the dotfiles launcher puts personal Macs in
  bypass mode, and the mode is hub's to decide.
- **A list of words as the launcher:** hub could check only the first word, so a missing script
  would show only when a tab opened.

## Risk

A launcher that drops or overrides hub's arguments can undo auto mode or the untrusted-input
guidance. The dotfiles launcher keeps a caller's `--permission-mode` and sends its own hints with
`--append-system-prompt-file` for that reason. hub checks neither: it trusts the launcher its own
`hub.toml` names. The dotfiles launcher also runs the work logins before `claude` starts, so on the
work laptop a tab hub opened in the background
([032](032-hubs-rex-tabs-open-in-their-repositorys-session.md)) can wait on a browser login.

## Revisit when

- An investigation needs to run through something that doesn't end in the `claude` CLI, which is
  also [015](015-accept-tmux-and-claude-code-coupling.md)'s trigger for revisiting the coupling.

## Also update

- `hub.example.toml`, and the line in [023](023-investigation-prompts-fence-foreign-text.md) that
  says investigations run `claude --permission-mode auto`.
