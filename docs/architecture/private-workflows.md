# Private Workflows and Skills

Hub is a public repo. Some workflows and investigation skills connect to systems you may not
want to name publicly (e.g. confidential work stuff) — those live in a separate private repo
(`hub-private`) that gets wired into this workspace via symlinks and a Cargo feature flag.

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

`just setup-private <device>` creates the standard symlinks inside hub:

```
hub/clients/src/private      →  hub-private/clients/src/
hub/workflows/src/private    →  hub-private/workflows/src/
hub/ui/cli/src/private       →  hub-private/ui/cli/src/
hub/ui/tui/src/private       →  hub-private/ui/tui/src/
hub/.env                     →  hub-private/devices/<device>.env
hub/hub.toml                 →  hub-private/devices/<device>.toml
```

It also creates individual file symlinks for private investigation modules:

```
hub/ui/tui/src/investigations/media.rs  →  hub-private/ui/tui/src/investigations/media.rs
```

Private skills follow the same individual-file pattern: skill files that
reference internal endpoints or queries live in `hub-private/.claude/skills/`
and are symlinked individually into `hub/.claude/skills/`. See the
[add-a-skill playbook](../playbooks/add-a-skill.md) for the full steps.

When adding a new private investigation module, add corresponding entries to
`scripts/setup-private.sh` and `.gitignore`.

**Device-specific modules require a stub on every other device.** The `private`
feature is enabled on every machine with hub-private, so `#[cfg(feature = "private")]`
alone is not sufficient — the file must also exist on every machine where that
feature is active, or the compiler (and `cargo fmt`) will error.

The pattern in `scripts/setup-private.sh`:

```bash
if [[ "$DEVICE" == "home-laptop" ]]; then
  link "$HUB_PRIVATE/ui/tui/src/investigations/media.rs" \
       "$HUB_ROOT/ui/tui/src/investigations/media.rs"
else
  stub "$HUB_ROOT/ui/tui/src/investigations/media.rs" \
    'use super::LaunchConfig;

pub(crate) fn config(_title: &str, _error: &str) -> LaunchConfig {
    unreachable!("media investigation not available on this device")
}'
fi
```

The stub implements the same API as the real module so compilation succeeds
on all devices. The `unreachable!()` body is never reached on machines where
the investigation is not configured. CI creates an empty stub (sufficient for
`cargo fmt --check`; CI never compiles with `--features private`).

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
