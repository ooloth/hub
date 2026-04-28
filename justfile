default:
    @just --list

# auto-enable private integrations when symlinks are in place
_features := if path_exists("clients/src/private") == "true" { "--features private" } else { "" }

# run a recipe with secrets injected from .env via 1Password
# usage: just op <recipe>
op +ARGS:
    op run --env-file=.env -- just {{ARGS}}

status:
    op run --env-file=.env -- cargo run -p hub-cli {{_features}} -- status

check:
    taplo fmt
    taplo check
    cargo fmt
    cargo clippy --fix --allow-dirty --allow-staged {{_features}}

build:
    cargo build {{_features}}

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
