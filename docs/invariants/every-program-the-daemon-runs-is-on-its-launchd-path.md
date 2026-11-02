# Every program the daemon runs is on its launchd PATH

## The invariant

Every program `hub-daemon`'s code starts by name is in `DAEMON_TOOLS`, the list whose directories
make up the `PATH` that `just daemon-start` writes into the daemon's LaunchAgent.

## Why it must hold

Under launchd the daemon does not inherit a shell's `PATH`. `scripts daemon start` builds one from
the directories where the caller's shell finds each program in `DAEMON_TOOLS`
(`scripts/src/daemon_agent/launch_agent.rs`), followed by `/usr/bin:/bin:/usr/sbin:/sbin`. Copying
the caller's whole `PATH` instead would carry per-session directories, such as fnm's, that differ
in every terminal, so every `just daemon-start` would rewrite the plist and restart the daemon.

A program the code starts that is not in the list is therefore found only if it happens to live in
a system directory. Otherwise the daemon fails to run it under launchd while it works from
`just daemon`, which reads the developer's own `PATH`. If the program is `op`, the daemon never
starts. If it is a source's tool, that source fails on every pass.

## What it forbids

- Starting a program by a new name in any crate `hub-daemon` is built from, without adding the
  name to `DAEMON_TOOLS`. A program is started by a start (`Command::new`,
  `disclaimed_spawn::spawn`) or by a wrapper that passes it on to one (`killed_on_drop`,
  `start_disclaimed`).
- A start or wrapper given its program as anything but a literal, or the first parameter of the
  function it sits in, because then no name is written where the check can read it.
- A function that passes its own parameter on to a start without being listed as a wrapper, and a
  listed wrapper that no longer does, because the check reads the program names where wrappers are
  called.
- Leaving a name in `DAEMON_TOOLS` after the code stops running it, which hands the daemon a
  directory it does not need.

It does not forbid starting a program by absolute path, which needs no `PATH`.

## How it is enforced

`daemon_tools_are_every_program_the_daemon_runs` in `scripts/src/repo_state/daemon_tools.rs`, run by
`just test`. It follows path dependencies in `cargo metadata` from `hub-daemon`, so a crate added to
the daemon later is covered without editing the test. It reads every `.rs` file under each of those
crates' `src/`, including hub-private's linked modules when they are present. Outside
`#[cfg(test)]`, it sorts every call to a start in `STARTS` or a wrapper in `WRAPPERS` by what it
is given as its program: a literal, which names a program the daemon runs; the first parameter of
the function the call sits in, which makes that function a wrapper; or anything else. It requires
the literals to equal `DAEMON_TOOLS`, the wrappers it finds to equal `WRAPPERS`, and nothing to be
anything else, naming each wrapper and each `file:line` that breaks the rule. It fails when it
finds no program at all.

What it misses:

- **A start or wrapper called by another name,** such as `spawn("op")` after
  `use disclaimed_spawn::spawn`, or `Command::new` through a type alias. The check reads spellings,
  not resolved paths.
- **A program a tool starts in turn,** such as the Python that `gcloud` runs. The daemon's `PATH`
  holds that program only when it shares a directory with the tool, as Homebrew's do.
- **Test code placed above a `#[cfg(test)]` line,** or production code below one, since the test
  cuts each file at the first such line.
- **hub-private modules not linked into this checkout,** which a device without them cannot read.
