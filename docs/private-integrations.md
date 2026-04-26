# Private Integrations

Hub is a public repo. Some integrations connect to systems you'd rather not name
publicly — they live in a separate private repo (`hub-private`) that gets wired into
this workspace via symlinks and a Cargo feature flag.

## How It Works

### The Two Repos

```
~/Repos/ooloth/
  hub/               ← public repo (this one)
  hub-private/       ← private companion repo
    clients/src/     ← private API clients
    workflows/src/   ← private workflows
    devices/         ← per-device configuration
      home-laptop.toml
      work-laptop.toml
    .env             ← 1Password secret references (shared across devices)
```

### Symlinks

`just setup-private <device>` creates four symlinks inside hub:

```
hub/clients/src/private   →  hub-private/clients/src/
hub/workflows/src/private →  hub-private/workflows/src/
hub/.env                  →  hub-private/.env
hub/hub.toml              →  hub-private/devices/<device>.toml
```

The first two are gitignored in hub. `.env` and `hub.toml` are also gitignored,
so none of the symlinks are ever committed to the public repo.

### Per-Device Configuration

Each device has its own file in `hub-private/devices/`. The `[integrations] enabled`
list controls which integrations run on that device — work integrations won't activate
on the home laptop if they're not listed in `home-laptop.toml`, and vice versa.

### Secrets

`.env` is shared across all devices — it holds op:// references for every integration.
Having unused references on a given device is harmless; `op run` only injects what's
present, and hub only reads what it needs.

### Cargo Feature Flag

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
individual integration clients as sub-modules. Same pattern for `workflows`.

## Initial Setup (per device)

```bash
# 1. Clone both repos (if not already done)
git clone git@github.com:ooloth/hub.git
git clone git@github.com:ooloth/hub-private.git ../hub-private

# 2. Add a device config to hub-private (if this device is new)
cp hub-private/devices/home-laptop.toml hub-private/devices/<this-device>.toml
# edit it — set device name and enabled integrations for this machine

# 3. Wire the symlinks
cd hub
just setup-private <this-device>

# 4. Verify everything compiles
just check
```

## Adding a Private Integration

1. Add `<name>.rs` to `hub-private/clients/src/` for the API client.
2. Add `pub mod <name>;` to `hub-private/clients/src/mod.rs`.
3. Add `<name>.rs` to `hub-private/workflows/src/` for the workflow.
4. Add `pub mod <name>;` to `hub-private/workflows/src/mod.rs`.
5. Add the integration's secrets to `hub-private/.env` (with 1Password references).
6. Add the integration's slug to the relevant `hub-private/devices/*.toml` files.
7. Run `just check` to confirm compilation.

## Recovering on a New Machine

```bash
git clone git@github.com:ooloth/hub.git
git clone git@github.com:ooloth/hub-private.git
cd hub
just setup-private <device-name>   # device config and .env are pulled from hub-private
just check
```

Private integrations, secrets references, and device configs are all preserved in
`hub-private`'s git history. The symlinks are the only connection between the two
repos at the filesystem level.
