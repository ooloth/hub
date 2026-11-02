# disclaimed_spawn

Starts a program that macOS holds responsible for itself, rather than for the process that started
it, with its stdout and stderr piped back. hub's only `unsafe` code lives here, and nothing else.

**Rules:**
- One job: the disclaimed spawn. Anything else belongs in the crate that needs it.
- Every `unsafe` block carries a `// SAFETY:` comment and an `#[allow(unsafe_code)]` where it is
  written. See [the invariant](../docs/invariants/unsafe-code-lives-only-in-disclaimed-spawn.md).
- Depends on no other hub crate.

## Why it exists

macOS judges what a process may reach by its *responsible* process, which a child inherits from
its parent. The daemon runs `op read` under launchd, and on macOS 27 an `op` whose responsible
process is the ad-hoc-signed `hub-daemon` cannot reach the 1Password desktop app: it reports "No
accounts configured for use with 1Password CLI". Spawned with responsibility disclaimed, `op` is
judged by 1Password's own signature and succeeds. [Decision
027](../docs/decisions/027-daemon-resolves-credentials-once-per-start-and-holds-them-in-memory.md)'s
addendum of 2026-11-01 holds the measurements.

Disclaiming goes through `responsibility_spawnattrs_setdisclaim`, a libSystem function Apple does not
document. It is looked up at runtime, so a macOS without it gives `Unsupported`, naming it, rather
than a binary that fails to load. hub runs only on macOS, so the crate fails to compile anywhere
else rather than fall back to a spawn that does not disclaim.

## Using it

```rust
let output = disclaimed_spawn::spawn("op", &["read", reference])?.output().await?;
```

`spawn` must run inside a tokio runtime. Dropping the `RunningChild`, or the future `output`
returns, before the child exits sends it `SIGKILL` and reaps it on a thread of its own, so a caller
enforces a deadline by dropping the future, as `config`'s credential deadline does.

## Seeing it work

`just test`, and CI on `macos-latest`, run the tests that only a real spawn can answer, in
`tests/spawn.rs`: that the child is its own responsible process (asked through
`responsibility_get_pid_responsible_for_pid`, with an ordinary spawn as the control), that a dropped
child is gone within 5 seconds, and that a descriptor the parent left inheritable never reaches the
child.

To see it in the daemon, install and restart it (`just install`, then `just daemon-restart`). One
Touch ID prompt naming `hub-daemon` appears, and `just daemon-logs` shows a `pass` line with
`outcome=ok` once it is approved.
