#!/usr/bin/env bash
# Fails if clippy fails in any private-module configuration this checkout can build.
#
# hub is compiled three ways: without hub-private, with it, and with it plus the
# home laptop's media module. A laptop compiles only the one it is set up for, so a
# change can pass there and fail on another device. This builds every configuration
# whose sources are present. See
# docs/invariants/hub-builds-with-and-without-each-private-module.md
set -euo pipefail

cd "$(dirname "$0")/.."

failed=()

lint() {
  local name="$1"
  shift
  echo "clippy: $name"
  if ! cargo clippy "$@" -- -D warnings; then
    failed+=("$name")
  fi
}

lint "without hub-private"

if [[ -e clients/src/private ]]; then
  lint "hub-private without media" --features private
fi

if [[ -e ui/tui/src/investigations/media.rs ]]; then
  lint "hub-private with media" -p hub-tui --features media
fi

if (( ${#failed[@]} > 0 )); then
  echo
  echo "clippy failed in: ${failed[*]}"
  echo
  echo "Every change must build on devices with and without each hub-private module,"
  echo "not only on the device it was written on. See"
  echo "docs/invariants/hub-builds-with-and-without-each-private-module.md"
  exit 1
fi
