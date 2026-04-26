default:
    @just --list

# run a recipe with secrets injected from .env via 1Password
# usage: just op <recipe>
op +ARGS:
    op run --env-file=.env -- just {{ARGS}}

check:
    cargo check

build:
    cargo build

cli:
    cargo run -p hub-cli

tui:
    cargo run -p hub-tui

test:
    cargo test

lint:
    cargo clippy

fmt:
    cargo fmt

clean:
    cargo clean
