---
number: 025
status: accepted
date: 2026-10-02
---

# 025 — Hub's own tooling is Rust, in the `scripts` crate

_Status: accepted; not yet implemented. `scripts/` holds five bash and two Python scripts and no
`Cargo.toml`._

## Forced by

`scripts/` holds seven tools, 360 lines in all: five in bash and two as `uv` single-file Python
scripts. None of them is tested or statically checked. `just test` reaches none of them, and
`just check` runs neither `ty` nor `shellcheck` (`justfile`, measured 2026-10-02). The Rust beside
them is held to clippy pedantic and 719 tests.

No record chose those languages. `scripts/check-script-shape.sh` enforces the `uv` shape on the
Python scripts that exist, which settles how a Python script is written, not whether one should be.

The gap became binding with a new tool. On 2026-10-02, twelve issue bodies and comments had to be
scrubbed of terms hub-private bans from hub, because nothing checks text posted through `gh`. The
guard that closes this is a Claude Code `PreToolUse` hook on every Bash call. A guard that fails
silently is the failure it exists to prevent, so it needs the same tests and static checks as the
code it protects.

## Decision

All logic in hub's own tooling is Rust, in one workspace crate at `scripts/`. The package and its
binary are both named `scripts`, following the plain names of the library crates; the `hub-*`
prefix marks binaries `cargo install` puts on a PATH, and this one is never installed.

Checks over the repository's own state, such as every crate having a README, lint inheritance and
`domain` staying pure, are `#[test]` functions, so `just test` runs them. Tools that act at a
particular moment, such as the Claude Code hook, the commit-time banned-terms check and
`setup-private`, are subcommands of the `scripts` binary.

Shell remains only where a command string is required: `justfile` recipes, `prek.toml` entries and
the hook command in `.claude/settings.json`.

The hook runs the prebuilt `target/debug/scripts`, not `cargo run`. Measured on this machine
2026-10-02: `cargo run` took about 0.63 s per call with nothing to rebuild, against under 0.01 s for
the binary, and a hook runs on every Bash call. `cargo run` also takes cargo's build-directory lock,
so it would wait whenever an agent is building (reasoned, not tested). The hook command denies when
the binary is missing, so a fresh clone or `cargo clean` blocks `gh` posts until the next build
rather than letting them through.

## Rejected

- **Keep bash and `uv` Python, hardened with `ty`, `shellcheck` and self-tests** — because it keeps
  three languages, three lint regimes and three test runners for 360 lines of tooling beside an
  all-Rust workspace, which is what every maintainer and agent then has to know. Its strongest case
  is real: it closes the testing gap with no port, and a warm `uv` script starts in about 0.03 s.
  Reverses if the application moves off Rust (see
  [the Textual question](../questions/should-the-tui-be-rebuilt-in-textual.md)); tooling follows
  the application's language.
- **Single-file Rust scripts with `cargo -Zscript`** — because it is nightly-only: cargo 1.95 on
  the pinned stable channel refuses `-Z` (verified 2026-10-02). Reverses if cargo script reaches
  stable.
- **Dev subcommands on the `hub` CLI** — because `hub` is the agent's toolkit
  ([Decision 010](010-hub-cli-as-agent-toolkit.md)), and dev tooling would ship inside it. Reverses
  if `hub` stops being a shipped binary.
- **One crate per tool** — because a hook has to run a binary that is present and current, and
  several binaries multiply what can be missing or stale. Reverses if one tool needs dependencies
  heavy enough to slow every other tool's build.
- **Not yet, deciding alongside the Textual question** — because that question is itself waiting on
  Decisions 019 to 022, and waiting leaves the guard untested in the meantime. Reverses if the
  Textual question resolves toward Python before porting starts.

## Risk

A prebuilt binary can be stale. After the hook's source changes, the hook runs the old logic until
the next build. The banned terms are read from hub-private at runtime, so the list itself is never
stale; only the matching logic can be.

The port touches a cross-repo call. `../hub-private/scripts/check-hub-builds.sh` runs
`$hub/scripts/check-private-configurations.sh` by path, so porting that script has to keep hub-private
working.

Seven tools are ported one per slice. Until the last one lands, `scripts/` mixes a crate with shell
and Python files.

## Revisit when

The application's language changes, or a tool has to run somewhere the Rust toolchain is not
installed.

## Also update

- [x] questions/README.md — no open question closes into this record.
- [x] vision.md — says nothing about hub's tooling; nothing to change.
- [x] `AGENTS.md` — the Project Structure entry for `scripts/` names the crate when it is created.
- [x] `scripts/README.md` — describes the crate when it is created.
- [x] `scripts/check-script-shape.sh` — deleted with the last Python script.
