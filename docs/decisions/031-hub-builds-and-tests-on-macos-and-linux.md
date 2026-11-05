---
number: 031
status: accepted
date: 2026-11-01
---

# 031 — hub builds and tests on macOS and Linux, and runs on macOS

## Forced by

- hub is developed in Amp orbs as well as on the account holder's Macs. An orb is a Linux sandbox,
  and its setup script (`.agents/setup`) builds the test binaries and runs `just lint` with every
  private module linked.
- Commit `0030f31` ([#389](https://github.com/ooloth/hub/issues/389)) made `disclaimed_spawn` fail
  to compile anywhere but macOS, on the premise that Linux was needed only for CI. Every crate that
  depends on `config` stopped building on Linux, so an orb could no longer set up, build, test or
  lint hub.
- The account holder, 2026-11-01: "we should ensure hub and hub-private work on both macos and
  linux".

## Decision

Every commit of hub, with or without each hub-private module, builds, passes clippy and passes its
tests on macOS and on Linux. CI builds, lints and tests every push to `main` and every pull
request on both, as a matrix of `macos-latest` and `ubuntu-latest`.

hub runs on macOS. What only macOS has stays macOS-only behind `cfg(target_os = "macos")`: the
disclaimed spawn of decision 027's addendum of 2026-11-01, launchd, and the notifications the
daemon posts. On Linux each of these compiles to nothing, does the nearest equivalent that needs
no macOS feature, or fails at runtime with an error naming what is missing. None of them has to
work end to end there. An orb uses hub to develop it, not to watch signals.

Code for a platform is kept to what the build needs. A platform difference lives in one place per
concept, so a reader finds it once rather than in every caller.

## Rejected

- **macOS only, as `0030f31` shipped.** Its strongest case is that hub only ever runs on macOS, so
  code for any other platform is code nobody runs, and CI tests the platform hub actually runs on.
  It is rejected because hub is developed on Linux too: with it, an orb cannot build hub at all.
  It would reverse if hub stopped being developed in orbs.
- **Linux checked only when an orb sets up, with CI on macOS alone.** Its strongest case is one CI
  runner per push and the orb as the real Linux environment. It is rejected because a change made
  on a Mac that breaks the Linux build is then found only by the next orb, which cannot build hub
  to fix it. It would reverse if orbs ran a check on every push of their own.
- **Linux checked from the Mac by building for `x86_64-unknown-linux-gnu`.** Its strongest case is
  that a broken Linux build is caught locally, before any push. It is rejected because a build for
  another target cannot run that target's tests, so it checks only half of what this record
  requires. It would reverse if the Mac could run Linux binaries as part of `just test`.

## Risk

CI runs twice per push. The runners work in parallel, so a push waits as long as the slower one,
the macOS runner. The repository is public, so the hosted runners cost nothing.

On Linux, the macOS-only features compile to stand-ins that nothing checks end to end. A daemon
started in an orb would start `op` without disclaiming, which is correct there because Linux holds
no process responsible for another, and its notifications would fail, because `terminal-notifier`
exists only on macOS.

Public CI has no hub-private, so it checks only the configuration without it, on both platforms.
The configurations with private modules are checked by `just lint` on whichever machine has them
linked: on a Mac before each commit, and in an orb at setup.

CPU is the only resource this touches, through the second CI run, and it does not bind. Memory,
storage and network are unchanged, because the binaries hub ships do not change on macOS.

## Revisit when

hub is no longer developed in Amp orbs, or hub needs a dependency that does not build on Linux.

## Also update

- [x] questions/README.md: no open question concerns which platforms hub builds on
- [x] vision.md: nothing defined or foreclosed; it names no platform
- [x] docs/invariants/hub-builds-with-and-without-each-private-module.md: add the platform axis,
      with the CI matrix as its check
- [x] AGENTS.md: the build rule names both platforms
