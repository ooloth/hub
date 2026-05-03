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
    workflows/src/   ← private workflows + PrivateStatusData types
    ui/cli/src/      ← private CLI rendering logic
    ui/tui/src/      ← private TUI rendering logic
    .claude/skills/  ← private investigation skills
    devices/         ← per-device configuration
      home-laptop.toml
      work-laptop.toml
    .env             ← 1Password secret references (shared across devices)
```

## Symlinks

`just setup-private <device>` creates six symlinks inside hub:

```
hub/clients/src/private      →  hub-private/clients/src/
hub/workflows/src/private    →  hub-private/workflows/src/
hub/ui/cli/src/private       →  hub-private/ui/cli/src/
hub/ui/tui/src/private       →  hub-private/ui/tui/src/
hub/.env                     →  hub-private/.env
hub/hub.toml                 →  hub-private/devices/<device>.toml
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

All four crates gate their `private` module behind the feature:

```rust
// clients/src/lib.rs and workflows/src/lib.rs
#[cfg(feature = "private")]
pub mod private;

// ui/cli/src/main.rs and ui/tui/src/main.rs
#[cfg(feature = "private")]
mod private;
```

`hub-private/clients/src/` is the `private` module for `clients`; it re-exports individual
clients as sub-modules. Same pattern for `workflows`, `ui/cli`, and `ui/tui`.

The rich domain types for private integrations (e.g. `PrivateStatusData`) live in
`hub-private/workflows/src/status.rs`. Hub's public crate only sees `PrivateStatusData`
as an opaque struct — it never imports integration-specific names. The CLI and TUI
rendering logic that knows the concrete fields lives in `hub-private/ui/cli/src/`
and `hub-private/ui/tui/src/` respectively.

## Playbooks

- [Set up the private workflows repository](../playbooks/set-up-private-workflows-repository.md) — first-time setup or recovery on a new machine
- [Add a private workflow](../playbooks/add-a-private-workflow.md) — wire in a new client and workflow
