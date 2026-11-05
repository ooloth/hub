# hub builds with and without each hub-private module

## The invariant

Every commit builds and passes clippy in every private-module configuration: without hub-private,
with hub-private but without the media module, and with both. It does so on macOS and on Linux,
and its tests pass on both
([decision 031](../decisions/031-hub-builds-and-tests-on-macos-and-linux.md)).

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

The same holds across platforms. hub runs on macOS but is also developed in Amp orbs, which are
Linux, and an orb's setup builds the tests and runs `just lint` with every private module linked.
Code that only macOS can compile, such as the disclaimed spawn of decision 027's addendum, leaves
an orb unable to build hub at all, and the orb is then the one place that cannot fix it.

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
- Code, a dependency or a test that compiles on only one of macOS and Linux without
  `cfg(target_os = …)` keeping it to that platform. What only macOS can do, such as
  `disclaimed_spawn`, compiles to nothing or to a stand-in elsewhere, never to a build error.

## How it is enforced

`scripts lint-configurations` (`scripts/src/lint_configurations.rs`) runs clippy with
`--all-targets -D warnings` once for each configuration whose sources are present in the checkout,
so test code is linted with the rest. `clippy.toml` lets `#[test]` functions and `#[cfg(test)]`
modules unwrap, expect, index and panic, and every other lint applies to them as it does to library
code. The script always builds without hub-private. It builds `--features private` when
`clients/src/private` exists. It builds `-p hub-tui --features media` when
`ui/tui/src/investigations/media.rs` exists. `just lint`
runs it, `just check` runs `just lint`, and prek runs `just lint` before every commit. When it
fails, it names the configurations that failed and points here.

Public CI checks only the first configuration, because it has no hub-private. It checks it on
both platforms: `.github/workflows/ci.yml` runs fmt, taplo, clippy, the tests, audit and deny on
`macos-latest` and on `ubuntu-latest` for every push to `main` and every pull request.

What the check misses:

- **A configuration whose sources are absent.** A laptop without the media symlink cannot build the
  media configuration, so a change made there to a caller of `media::config` is checked against
  the stub alone. The home laptop's next commit or `just check` catches it.
- **A private configuration on the other platform.** Public CI has no hub-private, so a private
  module that compiles on macOS but not on Linux is found only when an orb next sets up and runs
  `just lint`, and the reverse only on the next Mac commit.
- **Tests outside the checkout's fullest configuration.** `scripts lint-configurations` compiles
  every configuration's tests but runs none of them. `just test` runs them, by hand and before every
  push, in only the fullest configuration the checkout has, and public CI runs only the one without
  hub-private. So the private configurations' tests run on Linux only when an orb runs `just test`,
  and the tests of hub-private without the media module never run on a device that has it.
- **Dependabot's commits.** Dependabot commits on GitHub, where prek never runs `just lint`.
  AGENTS.md has a Dependabot PR that changes the Cargo files merged only after `just lint` and
  `just test` pass in a checkout with hub-private linked.
- **Runtime behaviour.** A stub that compiles can still do the wrong thing. `media_stub.rs` has a
  test asserting that it returns an error. That test runs only on a device without the media
  module.
