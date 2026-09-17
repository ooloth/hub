# `domain/` is pure

## The invariant

`domain/` computes only from its arguments: it performs no I/O, reads no ambient state, and imports
no other hub crate.

## Why it must hold

Every arrow in the dependency graph ends at `domain/`. [AGENTS.md](../../AGENTS.md) states the
direction as `ui/ → config/ → domain/` and `workflows/ → clients/ → domain/`, which only means
anything while `domain/` points at nothing. One reach back for config, a store or a client turns the
graph into a cycle and the rule into a slogan.

Purity is the other half of the same idea. A domain type that reads a clock or a file behaves
differently depending on when and where it is constructed, so it cannot be built identically in a
test, in the TUI, in a workflow, and in a daemon that does not exist yet. Being safe to depend on
from everywhere is the whole reason the crate exists.

## What it forbids

- Reading the environment, the filesystem, a process, a clock or a random source anywhere under
  `domain/src/`, including inside a constructor that looks pure from the outside.
- Adding `clients`, `config`, `store`, `workflows` or a UI crate to `domain/Cargo.toml`.
- Reaching ambient state through an existing dependency. `chrono` is a dependency for `Duration` as
  a value, not for `Utc::now()`.

What it permits, which is easy to confuse with the above: receiving ambient values as arguments. A
timestamp, a project name that originated in `hub.toml`, or a resolved credential passed in is
ordinary and common. What is forbidden is fetching one.

## How it is enforced

**The crate half is enforced by the compiler, totally.** `domain/Cargo.toml` declares only `chrono`,
`secrecy`, `serde`, `serde_json` and `uuid`, so a `use workflows::…` inside `domain/` does not
compile. There is no gap here and no check is needed.

**The ambient-state half has no compiler check**, because `std` is always in scope. That is what
`scripts/check-domain-is-pure.sh` is for: `prek` runs it on every commit and it fails if anything
under `domain/src/` matches the known ways in.

What that script misses: it matches names, not meaning. A clock reached through a future dependency
that spells the call differently, or an indirect call through a helper, walks straight past it. It
is a tripwire against the obvious reintroduction rather than a proof, and the honest reading is that
the compiler covers the half that can be covered totally while this half cannot be.
