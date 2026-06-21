# PR investigation worktrees

Hub creates git worktrees when a human opens a PR for investigation. The bare
repo at `~/.hub/repos/<project>/` is the parent; the worktree is a linked
checkout at `~/.hub/repos/<project>/pr-<N>/` on the PR's head branch.

Cleanup runs as part of `fetch::run()`: after each fetch, any worktree whose
remote tracking ref has disappeared (branch deleted on GitHub after merge or
close) is removed automatically. See `workflows/src/fetch.rs`.
