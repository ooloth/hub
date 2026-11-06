# PR investigation worktrees

Hub creates git worktrees when a human opens a PR for investigation. The bare
repo at `~/.hub/repos/<repo name>/` is the parent; the worktree is a linked
checkout at `~/.hub/repos/<repo name>/pr-<N>/` on the PR's head branch.

Cleanup runs as part of `fetch::run()`, which only the TUI's own refreshes call; the daemon's
refreshes do not. After each fetch, any worktree whose
remote tracking ref has disappeared (branch deleted on GitHub after merge or
close) is removed automatically. See `workflows/src/fetch.rs`.

CI, issue, Loki and GCP investigations run in detached worktrees instead, at
`~/.hub/repos/<repo name>/investigation-<timestamp>/`. That cleanup skips them,
because they have no branch. The investigation's window removes its worktree
once the agent exits, and the launch removes it with
`fetch::remove_investigation_worktree` when the window fails to open. A window
closed before the agent exits leaves its worktree behind.
