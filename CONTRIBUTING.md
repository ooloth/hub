# Contributing

## Prerequisites

- [Rust](https://rustup.rs)
- [just](https://github.com/casey/just) — `brew install just`
- [1Password CLI](https://developer.1password.com/docs/cli) — `brew install 1password-cli`

## Setup

Hub can run in two modes depending on whether you have access to `hub-private`.

### Standalone (public integrations only)

`hub-private` is not required. Without it, hub compiles and runs with public
integrations only (e.g. GitHub PRs). The `private` feature is silently skipped.

```bash
git clone <repo> && cd hub
cp .env.example .env
# edit .env — replace op:// references with your actual 1Password paths
just check
```

`.env` and `hub.toml` live as plain local files in the repo root, gitignored.

### With hub-private (adds private integrations)

If you have access to `hub-private`, it replaces the plain `.env` and `hub.toml`
files with symlinks into the private repo, and adds private integration code.

> If you already created local `.env` or `hub.toml` files above, remove them
> before running this — `setup-private` will error rather than overwrite them.

```bash
git clone git@github.com:ooloth/hub-private.git ../hub-private
just setup-private <device>   # e.g. just setup-private home-laptop
just check
```

`<device>` must match a file in `hub-private/devices/<device>.toml`. That file
controls which integrations are active on this machine — work integrations won't
activate on the home laptop if they're not listed there.

See [docs/private-integrations.md](docs/private-integrations.md) for the full
model, how to add new devices, and how to add new private integrations.

## Running

```bash
just check              # verify workspace compiles
just op status          # run with secrets injected
```

## Common tasks

```bash
just fmt                # format code
just lint               # run clippy
just test               # run all tests
just build              # build all crates
```
