# hub builds with and without each hub-private module

## The invariant

Every commit builds and passes clippy in every private-module configuration: without hub-private,
with hub-private but without the media module, and with both.

## Why it must hold

Each laptop compiles exactly one of those configurations. The one it compiles depends on which
hub-private symlinks `just setup-private <device>` created there. A change that compiles on the
laptop it was written on can still fail to compile on another. The device that finds out is the one
that ran `just install`, and it is left without a working `hub-tui` until someone fixes the code on
a machine that can reach the failure.

The risky part is any interface a private module shares with hub. `investigations::media::config`
has a real implementation in hub-private and a stub in `ui/tui/src/investigations/media_stub.rs`.
hub calls it with the same arguments in both cases, so the two signatures must agree. The home
laptop compiles only the real one and the work laptop compiles only the stub. A signature change
made on one laptop therefore fails to compile on the other.

## What it forbids

- A change to the real `media.rs` signature without the same change to `media_stub.rs`.
- A stub, placeholder or shared signature that lives anywhere the compiler does not read in some
  configuration, such as a template string inside a script or a generated file outside git.
- A module that compiles only because a symlink happens to exist on the author's laptop. Its
  `mod` declaration sits behind the feature that enables it.
- A shared signature that names a private service. It is public in hub through the stub and the
  call site. The generic `Media*` vocabulary is what hub uses instead.
- A device-specific module added without a feature to select it, a tracked stub, and a
  configuration in `scripts/src/lint_configurations.rs`.

## How it is enforced

`scripts lint-configurations` (`scripts/src/lint_configurations.rs`) runs clippy with
`-D warnings` once for each configuration whose sources are present in the checkout. It always
builds without hub-private. It builds `--features private` when `clients/src/private` exists. It
builds `-p hub-tui --features media` when `ui/tui/src/investigations/media.rs` exists. `just lint`
runs it, `just check` runs `just lint`, and prek runs `just lint` before every commit. When it
fails, it names the configurations that failed and points here.

Public CI checks only the first configuration, because it has no hub-private.

What the check misses:

- **A configuration whose sources are absent.** A laptop without the media symlink cannot build the
  media configuration, so a change made there to a caller of `media::config` is checked against
  the stub alone. The home laptop's next commit or `just check` catches it.
- **Test code.** The check lints library and binary targets, not tests, because hub's test code has
  never been linted and fails clippy's `unwrap_used` and `indexing_slicing` rules. A test that
  compiles only in one configuration fails in `just test`, which runs by hand and before a push.
- **Runtime behaviour.** A stub that compiles can still do the wrong thing. `media_stub.rs` has a
  test asserting that it returns an error. That test runs only on a device without the media
  module.
