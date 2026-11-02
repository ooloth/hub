# `unsafe` code lives only in disclaimed_spawn

## The invariant

No crate in the workspace except `disclaimed_spawn` can compile an `unsafe` block, and
`disclaimed_spawn`'s lints are the workspace's in every other respect.

## Why it must hold

hub holds credentials in memory for the life of the daemon
([Decision 027](../decisions/027-daemon-resolves-credentials-once-per-start-and-holds-them-in-memory.md)).
Safe Rust rules out the memory errors that could read or corrupt them, so the code that handles
them can be reviewed for logic alone. One `unsafe` block anywhere in that code would make every
reviewer of it check memory safety too, and nothing would tell them to.

`disclaimed_spawn` needs `unsafe` to call libSystem: the function that disclaims responsibility has
no safe wrapper that can pipe a child's output, as the decision's addendum of 2026-11-01 records.
Keeping those calls in one crate whose only job is the spawn means one small place to review for
memory safety, and every crate that touches a credential keeps the compiler's guarantee.

## What it forbids

- An `unsafe` block, `unsafe impl` or `extern` call in any crate but `disclaimed_spawn`, including
  one in a test.
- A crate that drops `[lints] workspace = true`, which would leave it without the workspace's
  `unsafe_code = "forbid"`.
- `disclaimed_spawn` relaxing `unsafe_code` beyond `"deny"`, or carrying lints that differ from the
  workspace's in any other way, so its code is held to less than the rest of the tree.
- Growing `disclaimed_spawn` beyond the spawn, because whatever moves there escapes the `forbid`.

## How it is enforced

The workspace sets `unsafe_code = "forbid"`, which no attribute inside a crate can override, so any
crate that inherits the workspace lints fails to compile an `unsafe` block.
`every_workspace_member_inherits_the_workspace_lints`
(`scripts/src/repo_state/lint_inheritance.rs`) checks that every member inherits them, and that
`disclaimed_spawn` carries an exact copy of them with only `unsafe_code` changed to `"deny"`. It
fails when `disclaimed_spawn` is missing from the workspace, so a rename cannot skip it. `just test`
runs it.

What the check misses:

- **What `disclaimed_spawn` does with `unsafe`.** Each block is allowed where it is written and
  carries a `// SAFETY:` comment, which clippy's `undocumented_unsafe_blocks` requires, but whether
  the comment is true is for review.
- **A dependency's `unsafe`.** Third-party crates are outside the workspace lints.
- **The fourth item above.** Whether code in `disclaimed_spawn` is still only the spawn is for
  review.
