# Should investigations run with permissions skipped?

Every investigation hub launches runs `claude --dangerously-skip-permissions`, built in
`ui/tui/src/investigations/command.rs`. The only scoping is the per-type `--allowedTools` string,
and the narrowest of those is `Bash`, which is not a restriction.

## Why it matters

[Decision 023](../decisions/023-investigation-prompts-fence-foreign-text.md) reduces how often an
agent follows an instruction that came from outside hub. It does nothing about what happens when one
is followed. Those are the two halves of the same exposure, and only one of them has been addressed.

The second half is the one that decides outcomes. An investigation runs in a worktree of a real
repository, on the machine that holds the credentials, with unattended Bash. The blast radius of a
successful injection is the same whether the injection was likely or unlikely.

This also gates how far hub can go unattended. [#327](https://github.com/ooloth/hub/issues/327)
moves hub toward a daemon that notices things without being asked, and the epic's plan of record
already lists auto-triggered investigations as out of scope. Part of why they are out of scope is
that nobody is watching when one runs. Answering this question is what would make that
reconsiderable.

## What would settle it

An observation of what actually happens when permissions are not skipped. Claude Code's approval
prompt is interactive, and an investigation opens in a tmux window the person may not be looking at,
so the question is what a pending approval does to a window nobody is watching: whether it blocks
silently, whether it is discoverable from the TUI, and how long a session sits unattended before the
approval is stale.

That is a spike, not an argument: launch one investigation without the flag, put an approval-
requiring step in its path, and watch the window. Until that is observed, every option below is a
guess about the ergonomics.

Also needed: what the realistic attacker actually reaches. Media blocked-import titles derive from
release names, which come from the internet, and that is the only field in hub today that a stranger
writes without going through the user's own systems first. Whether the other fields are reachable
determines how much this matters.

## Resolves into

A decision record, if the answer changes what an investigation is allowed to do. A change that only
narrows the per-type `--allowedTools` strings is a value edit and would close into the issue that
makes it.

## Source

Raised while implementing [#310](https://github.com/ooloth/hub/issues/310), which fenced
externally-authored text in prompts and explicitly scoped the permission question out. Recorded in
023's Risk section as knowingly accepted.

## Options

**Keep skipping permissions.** Strongest case: it is why investigations are useful unattended. An
agent that stops to ask cannot be launched and left, which is the whole ergonomic that makes
pressing `i` worth doing. Cost: a successful injection has unrestricted Bash on the machine holding
the credentials, and nothing between it and the filesystem.

**Narrow `--allowedTools` per investigation type.** Strongest case: cheap, local, and several types
genuinely need less than they are given. A log investigation reads and greps; it has no reason to
write. Cost: `Bash` is the tool every type needs and the one that makes the others redundant, so
this improves the paperwork more than the exposure unless Bash itself is constrained.

**Stop skipping permissions and handle approvals.** Strongest case: it is the only option that
changes what happens after an injection succeeds rather than how often one does. Cost: unknown, and
that is the point of the spike. An approval prompt in an unwatched tmux window may be strictly worse
than no prompt, because the session looks running and is not.

**Not yet.** Strongest case: nothing in the next milestone needs the answer, and deciding now means
deciding without whatever Phases 3 to 6 teach about how hub behaves unattended. Cost: the exposure
stays, and the longer investigations are launched this way the more the ergonomic is assumed.

## Findings

Nothing here is settled until it graduates into a decision record.

- The per-type tool strings today are `Bash` for CI and the private media type, `Bash,Read` for
  issue, Loki and GCP, and `Bash,Read,Edit,Write,Glob,Grep` for both PR types. *Measured*, read from
  `ui/tui/src/investigations/` on 2026-09-19.
- A prompt-injection attempt in a Loki message and in a media blocked-import title both reached the
  agent inert and fenced, with no shell execution. *Measured*, by injecting rows into the status
  cache and reading the launched process's environment on 2026-09-19.
- Delimiter-based prompt defences reduce injection success rates without eliminating them.
  *Unverified*: stated from general knowledge of the literature, with no benchmark run against this
  fence or citation checked.
