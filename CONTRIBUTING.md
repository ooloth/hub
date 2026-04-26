# Contributing

## Prerequisites

- [Rust](https://rustup.rs)
- [just](https://github.com/casey/just) — `brew install just`
- [1Password CLI](https://developer.1password.com/docs/cli) — `brew install 1password-cli`

## Setup

```bash
git clone <repo> && cd hub
cp .env.example .env
# edit .env — replace op:// references with your actual 1Password paths
just check
```

## Running

```bash
just check              # verify workspace compiles
just op status          # run with secrets injected
```

## Private integrations (optional)

Hub supports private integrations via a companion repo. If you have access to
`hub-private`, wire it in:

```bash
git clone git@github.com:ooloth/hub-private.git ../hub-private
just setup-private
```

The symlinks are detected automatically — `just check` will include private code
once they exist. See [docs/private-integrations.md](docs/private-integrations.md)
for the full setup walkthrough and how to add new private integrations.

## Common tasks

```bash
just fmt                # format code
just lint               # run clippy
just test               # run all tests
just build              # build all crates
```
