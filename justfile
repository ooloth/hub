default:
    @just --list

# auto-enable private integrations when symlinks are in place
# The `private` feature enables the private workflow integrations defined in `hub-private`.
# Without it, only the public integrations defined in `hub` are shown.
_features := if path_exists("clients/src/private") == "true" { "--features private" } else { "" }

status:
    cargo run -p hub-cli {{_features}} -- status

check:
    taplo fmt
    taplo check
    cargo fmt
    cargo clippy --fix --allow-dirty --allow-staged {{_features}} -- -D warnings

build:
    cargo build {{_features}}

install:
    cargo install --path ui/tui {{_features}}

cli:
    cargo run -p hub-cli {{_features}}

tui:
    cargo run -p hub-tui {{_features}}

test:
    cargo nextest run {{_features}}

test-update:
    INSTA_UPDATE=always cargo nextest run {{_features}}

mutants:
    cargo mutants {{_features}}

lint:
    cargo clippy {{_features}}

fmt:
    cargo fmt
    taplo fmt

clean:
    cargo clean

# wire hub-private into this repo (run once per device after cloning hub-private)
# DEVICE must match a file in hub-private/devices/<device>.toml
setup-private DEVICE HUB_PRIVATE_PATH="../hub-private":
    bash scripts/setup-private.sh {{DEVICE}} {{HUB_PRIVATE_PATH}}
