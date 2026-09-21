default:
    @just --list

# auto-enable private integrations when symlinks are in place
# The `private` feature enables the private workflow integrations defined in `hub-private`.
# Without it, only the public integrations defined in `hub` are shown.
_features := if path_exists("clients/src/private") == "true" { "--features private" } else { "" }

# Every recipe that runs hub from source uses the `dev` profile, so development
# never writes the database the installed hub reads. Override per invocation
# with `HUB_PROFILE=default just tui`. The installed binaries get `default` by
# not having this set — a login shell does not export it and launchd never
# inherits one. See docs/decisions/024-hub-state-is-per-profile.md
export HUB_PROFILE := env_var_or_default("HUB_PROFILE", "dev")

status:
    cargo run -p hub-cli {{_features}} -- status

check:
    @scripts/check-crate-readmes.py
    @scripts/check-lint-inheritance.py
    @scripts/check-script-shape.sh
    taplo fmt
    taplo check
    cargo fmt
    cargo clippy --fix --allow-dirty --allow-staged {{_features}} -- -D warnings
    cargo clippy {{_features}} -- -D warnings

build:
    cargo build {{_features}}

install:
    # FIXME: cargo install --path ui/cli {{_features}}
    cargo install --path ui/tui {{_features}}

cli:
    cargo run -p hub-cli {{_features}}

tui:
    cargo run -p hub-tui {{_features}}

# run one refresh with nobody present, then exit
daemon:
    cargo run -p hub-daemon {{_features}}

db:
    @echo "opening ~/.hub/$HUB_PROFILE/hub.db"
    uvx visidata ~/.hub/"$HUB_PROFILE"/hub.db

# seed one synthetic signal into the status cache for manual QA (KIND: loki, gcp, media-blocked, ci)
qa-seed KIND *ARGS:
    scripts/seed-signal.py {{ KIND }} {{ ARGS }}

# put the real status cache back after qa-seed
qa-restore:
    scripts/seed-signal.py --restore

_require-nextest:
    @cargo nextest --version > /dev/null 2>&1 || (echo "error: cargo-nextest not installed — run: cargo install cargo-nextest --locked" && exit 1)

test: _require-nextest
    cargo nextest run {{_features}}

test-update: _require-nextest
    INSTA_UPDATE=always cargo nextest run {{_features}}

_require-mutants:
    @cargo mutants --version > /dev/null 2>&1 || (echo "error: cargo-mutants not installed — run: cargo install cargo-mutants --locked" && exit 1)

mutants: _require-mutants
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
