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

## Common tasks

```bash
just fmt                # format code
just lint               # run clippy
just test               # run all tests
just build              # build all crates
```
