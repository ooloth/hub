# Contributing

## Prerequisites

- [Rust](https://rustup.rs)
- [just](https://github.com/casey/just) — `brew install just`
- [1Password CLI](https://developer.1password.com/docs/cli) — `brew install 1password-cli`
- [taplo](https://taplo.tamasfe.dev) — `brew install taplo` (TOML formatter and schema validator)
- [prek](https://github.com/j178/prek) — `brew install prek` (git hook manager)

## Setup

Hub can run in two modes depending on whether you have access to `hub-private`.

### Standalone (public workflows only)

`hub-private` is not required. Without it, hub compiles and runs with public
workflows only (e.g. GitHub PRs). The `private` feature is silently skipped.

```bash
git clone <repo> && cd hub
cp .env.example .env
# edit .env — replace op:// references with your actual 1Password paths
prek install
just check
```

`.env` and `hub.toml` live as plain local files in the repo root, gitignored.

### With hub-private (adds private workflows)

If you have access to `hub-private`, it replaces the plain `.env` and `hub.toml`
files with symlinks into the private repo, and adds private workflow code.

> If you already created local `.env` or `hub.toml` files above, remove them
> before running this — `setup-private` will error rather than overwrite them.

```bash
git clone git@github.com:ooloth/hub-private.git ../hub-private
just setup-private <device>   # e.g. just setup-private home-laptop
just check
```

`<device>` must match a file in `hub-private/devices/<device>.toml`. That file
controls which workflows are active on this machine — work workflows won't
activate on the home laptop if they're not listed there.

See [docs/architecture/private-workflows.md](docs/architecture/private-workflows.md)
for the full model, how to add new devices, and how to add new private workflows.

## Running

```bash
just check              # verify workspace compiles
just status             # run with secrets injected
```

## Common tasks

```bash
just fmt                # format code
just lint               # run clippy
just test               # run all tests
just build              # build all crates
```

## Playbooks

- [Add a project](docs/playbooks/add-a-project.md) — add a codebase to your device config
- [Add a workflow](docs/playbooks/add-a-workflow.md) — implement a new workflow end-to-end
