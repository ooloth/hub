# Private Workflows and Prompts

Hub is a public repo. Some workflows and investigation prompts connect to systems you may not
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
    prompts/         ← private investigation prompts
    devices/         ← per-device configuration
      home-laptop.toml   ← includes [credentials] with op:// references
      work-laptop.toml
```

## Symlinks

`just setup-private <device>` creates the standard symlinks inside hub:

```
hub/clients/src/private      →  hub-private/clients/src/
hub/workflows/src/private    →  hub-private/workflows/src/
hub/ui/cli/src/private       →  hub-private/ui/cli/src/
hub/ui/tui/src/private       →  hub-private/ui/tui/src/
hub/hub.toml                 →  hub-private/devices/<device>.toml
```

On the home laptop it also links the one device-specific investigation module:

```
hub/ui/tui/src/investigations/media.rs  →  hub-private/ui/tui/src/investigations/media.rs
```

The prompt that module loads is a symlink tracked in hub,
`hub/prompts/investigations/media.md → ../../../hub-private/prompts/media-investigate.md`.
Git stores only the target path, so the prompt text stays in hub-private, and the link resolves
wherever hub-private is checked out beside hub.

**A device-specific module has a tracked stub, selected by a cargo feature.** The `media` feature
on `hub-tui` compiles the real `media.rs`. Every other `private` build compiles
`investigations/media_stub.rs`, which has the same signature and returns an error saying the
investigation is not available on this device. `investigations/mod.rs` names both paths in
`cfg_attr`, so rustfmt formats whichever exists and does not fail when `media.rs` is absent.

The stub has to keep matching the real signature, and `just lint` checks that by compiling every
configuration the checkout can build. See
[the invariant](../invariants/hub-builds-with-and-without-each-private-module.md).

When adding a new device-specific module, add its link to `scripts/src/setup_private.rs` and
`.gitignore`, a feature to select it, a tracked stub, and a configuration in
`scripts/src/lint_configurations.rs`.

All of these are gitignored in hub, so none of the symlinks are ever committed
to the public repo.

## Per-Device Configuration

Each device has its own file in `hub-private/devices/`. It lists the `[[project]]`
entries and their `[[project.workflow]]` / `[[project.environment]]` blocks relevant
to that machine — work projects won't activate on the home laptop if they're not
listed in `home-laptop.toml`, and vice versa.

## Credentials

Each device file includes a `[credentials]` table with the `op://` references it
needs. Unknown keys (private workflow credentials like `media_server_url`) are captured
in `Config.extra_credentials` and passed to hub-private workflows — the public hub
code never sees the key names. Having extra keys on a device is harmless; hub only
reads what it needs.

## Cargo Feature Flag

The `private` feature is declared in `clients/Cargo.toml` and `workflows/Cargo.toml`.
When the symlinks exist, the justfile detects them and passes `--features private`
automatically to every `cargo` invocation. You never need to remember to pass it.

The same detection adds `hub-tui/media` when the home laptop's `media.rs` link exists. See
`_features` in the justfile.

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
