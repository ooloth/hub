# Contributing

## Prerequisites

- [Rust](https://rustup.rs)
- [just](https://github.com/casey/just) — `brew install just`
- [1Password CLI](https://developer.1password.com/docs/cli) — `brew install 1password-cli`
- [taplo](https://taplo.tamasfe.dev) — `brew install taplo` (TOML formatter and schema validator)
- [prek](https://github.com/j178/prek) — `brew install prek` (git hook manager)
- [cargo-nextest](https://nexte.st) — `cargo install cargo-nextest --locked` (test runner used by `just test`)

## Setup

Hub can run in two modes depending on whether you have access to `hub-private`.

### Standalone (public workflows only)

`hub-private` is not required. Without it, hub compiles and runs with public
workflows only (e.g. GitHub PRs). The `private` feature is silently skipped.

```bash
git clone <repo> && cd hub
cp hub.toml.example hub.toml
# edit hub.toml — fill in [credentials] with your 1Password references or plain values
prek install
just check
```

`hub.toml` lives as a plain local file in the repo root, gitignored.

### With hub-private (adds private workflows)

If you have access to `hub-private`, it replaces the plain `hub.toml`
file with a symlink into the private repo, and adds private workflow code.

> If you already created a local `hub.toml` file above, remove it before
> running this — `setup-private` will error rather than overwrite it.

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
just check              # fmt + lint (autofixes where possible)
just status             # run the CLI status command
```

## Common tasks

```bash
just fmt                # format code
just lint               # run clippy
just test               # run all tests
just build              # build all crates
```

## Verifying a change by hand

Tests are not enough for anything that affects keybindings, navigation,
subprocess launching, tmux integration or the cache format. Those are verified
by running the TUI and driving it.

Signals come from live APIs, so you cannot choose the text of a real one.
`just qa-seed` writes a synthetic signal into the status cache and
`just qa-restore` puts the real one back:

```bash
just qa-seed loki       # or gcp, media-blocked, ci
just qa-restore         # always, including when a run fails partway
```

Seeding backs the database up first and refuses to run twice, so a forgotten
restore cannot cost you the real cache.

[AGENTS.md](AGENTS.md) has the full loop, including how to read what an
investigation was actually launched with, and the handful of tmux and 1Password
behaviours that otherwise look like bugs in your change.

## Playbooks

- [Add a project](docs/playbooks/add-a-project.md) — add a codebase to your device config
- [Add a workflow](docs/playbooks/add-a-workflow.md) — implement a new workflow end-to-end
