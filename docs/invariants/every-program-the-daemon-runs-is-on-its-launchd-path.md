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

- Adding a `Command::new("…")` or `killed_on_drop("…")` with a new program name to any crate
  `hub-daemon` is built from, without adding the name to `DAEMON_TOOLS`.
- Leaving a name in `DAEMON_TOOLS` after the code stops running it, which hands the daemon a
  directory it does not need.

It does not forbid starting a program by absolute path, which needs no `PATH`.

## How it is enforced

`daemon_tools_are_every_program_the_daemon_runs` in `scripts/src/repo_state/daemon_tools.rs`, run by
`just test`. It follows path dependencies in `cargo metadata` from `hub-daemon`, so a crate added to
the daemon later is covered without editing the test. It reads every `.rs` file under each of those
crates' `src/`, including hub-private's linked modules when they are present. Then it requires the
literal program names in `Command::new("…")` and `killed_on_drop("…")` outside `#[cfg(test)]` to
equal `DAEMON_TOOLS`, and fails when it finds none at all.

What it misses:

- **A program named by a variable,** such as `Command::new(program)` in `clients/src/gcp.rs`. The
  test sees only the literal at the call site that supplies the name, and only if that call site
  uses one of the two spellings above.
- **A program a tool starts in turn,** such as the Python that `gcloud` runs. The daemon's `PATH`
  holds that program only when it shares a directory with the tool, as Homebrew's do.
- **Test code placed above a `#[cfg(test)]` line,** or production code below one, since the test
  cuts each file at the first such line.
- **hub-private modules not linked into this checkout,** which a device without them cannot read.
