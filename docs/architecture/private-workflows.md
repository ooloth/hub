# Private Workflows and Skills

Hub is a public repo. Some workflows and investigation skills connect to systems
you'd rather not name publicly — they live in a separate private repo (`hub-private`)
that gets wired into this workspace via symlinks and a Cargo feature flag.

## The Two Repos

```
~/Repos/ooloth/
  hub/               ← public repo (this one)
  hub-private/       ← private companion repo
    clients/src/     ← private API clients
    workflows/src/   ← private workflows
    .claude/skills/  ← private investigation skills
    devices/         ← per-device configuration
      home-laptop.toml
      work-laptop.toml
    .env             ← 1Password secret references (shared across devices)
```

## Symlinks

`just setup-private <device>` creates four symlinks inside hub:

```
hub/clients/src/private   →  hub-private/clients/src/
hub/workflows/src/private →  hub-private/workflows/src/
hub/.env                  →  hub-private/.env
hub/hub.toml              →  hub-private/devices/<device>.toml
```

Private skills follow the same principle: skill files that reference internal
endpoints or queries live in `hub-private/.claude/skills/` and are symlinked
individually into `hub/.claude/skills/`. See the
[add-a-skill playbook](../playbooks/add-a-skill.md) for the full steps.

The first two are gitignored in hub. `.env` and `hub.toml` are also gitignored,
so none of the symlinks are ever committed to the public repo.

## Per-Device Configuration

Each device has its own file in `hub-private/devices/`. It lists the `[[project]]`
entries and their `[[project.workflow]]` / `[[project.environment]]` blocks relevant
to that machine — work projects won't activate on the home laptop if they're not
listed in `home-laptop.toml`, and vice versa.

## Secrets

`.env` is shared across all devices — it holds `op://` references for every workflow.
Having unused references on a given device is harmless; `op run` only injects what's
present, and hub only reads what it needs.

## Cargo Feature Flag

The `private` feature is declared in `clients/Cargo.toml` and `workflows/Cargo.toml`.
When the symlinks exist, the justfile detects them and passes `--features private`
automatically to every `cargo` invocation. You never need to remember to pass it.

```just
_features := if path_exists("clients/src/private") == "true" { "--features private" } else { "" }
```

Both crates gate their `private` module behind the feature:

```rust
// clients/src/lib.rs
#[cfg(feature = "private")]
pub mod private;

// workflows/src/lib.rs
#[cfg(feature = "private")]
pub mod private;
```

`hub-private/clients/src/` is the `private` module for `clients`; it re-exports
individual clients as sub-modules. Same pattern for `workflows`.

## Playbooks

- [Set up the private workflows repository](../playbooks/set-up-private-workflows-repository.md) — first-time setup or recovery on a new machine
- [Add a private workflow](../playbooks/add-a-private-workflow.md) — wire in a new client and workflow
