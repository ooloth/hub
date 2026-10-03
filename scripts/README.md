# scripts

Hub's own dev and ops tooling. Not part of any binary hub ships.

[Decision 025](../docs/decisions/025-hub-tooling-is-rust-in-the-scripts-crate.md) puts all of it
in this crate. Checks over the repository's own state are `#[test]` functions, so `just test` runs
them. Tools that act at a particular moment are subcommands of the `scripts` binary. The shell and
Python files still here are waiting to be ported, one per change.

**Lives here:** repository checks, Claude Code hooks, local setup, device bootstrapping, one-off
data tasks.

Not imported by anything.

## Subcommands

- `guard-gh-posts --banned-terms <path>` — the Claude Code `PreToolUse` hook wired in
  `.claude/settings.json`. Refuses a `gh` call that would publish a term from hub-private's
  `scripts/blocked-terms.txt`: in the command text, a heredoc, or a body file. Also refuses a body
  it cannot read, such as a path behind `$VAR`. Allows everything when the list does not exist,
  which is every checkout without hub-private beside it.
- `check-staged-terms` — the pre-commit check prek runs. Refuses a commit whose staged content
  holds a term from hub-private's list, naming each file and term. Reads the git index, not the
  working copy, and finds the list beside the main checkout, so it works in worktrees too.
- `lint-configurations` — what `just lint` runs. Clippy with `-D warnings` once per configuration
  whose sources this checkout has: without hub-private always, then with each linked module. Runs
  them all, then names the failures. hub-private's pre-commit check calls it too.

## Running it

The hook runs the prebuilt `target/debug/scripts`, never `cargo run`, which measured about 0.63 s
per call against under 0.01 s and would wait on any build in progress. So build after changing it:

```bash
just build
```

Until the binary exists, the hook command in `.claude/settings.json` refuses any Bash call that
mentions `gh` (when hub-private is present) and allows everything else, so `just build` itself
still runs.

To see the guard decide without Claude Code, pipe it a hook request:

```bash
echo '{"tool_name":"Bash","cwd":".","tool_input":{"command":"gh issue comment 1 -b hello"}}' \
  | target/debug/scripts guard-gh-posts --banned-terms ../hub-private/scripts/blocked-terms.txt
```

No output means allowed. A refusal prints the hook's deny JSON with the reason.

## Files

- `src/main.rs` — the subcommands
- `src/repo_state/` — checks over the repository's own state, compiled only for tests:
  `crate_readmes` (every workspace member has a non-empty README), `lint_inheritance` (every
  member's manifest sets `[lints] workspace = true`), `domain_purity` (nothing under `domain/src`
  names a way of reading ambient state) and `workspace` (the member list, from `cargo metadata`).
  Run them alone with `cargo nextest run -p scripts -E 'test(repo_state)'`
- `src/banned_terms.rs`, `src/scanned_text.rs` — the one matching rule the guard and the
  pre-commit check share
- `src/staged_terms.rs` — the pre-commit check
- `src/lint_configurations.rs` — the configuration lint
- `src/repo_root.rs` — the checkout this binary was built from
- `src/gh_post_guard/` — the guard, one concept per file: `shell_words`, `publishing_call`,
  `body_source`, `verdict`, `hook_io`, and `respond`, which joins them
