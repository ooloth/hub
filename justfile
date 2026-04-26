default:
    @just --list

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
