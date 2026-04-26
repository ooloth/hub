#!/usr/bin/env bash
set -euo pipefail

# Wire hub-private into this repo via symlinks.
# Run once per device after cloning hub-private alongside hub.
#
# Usage: bash scripts/setup-private.sh <device-name> [path-to-hub-private]
# Or:    just setup-private <device-name>
#
# <device-name> must match a file in hub-private/devices/<device-name>.toml

DEVICE="${1:-}"
HUB_PRIVATE="${2:-../hub-private}"
HUB_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [[ -z "$DEVICE" ]]; then
  echo "error: device name required"
  echo "usage: just setup-private <device-name>"
  echo ""
  echo "available devices:"
  ls "$HUB_PRIVATE/devices/" 2>/dev/null | sed 's/\.toml$/  /' || echo "  (none yet)"
  exit 1
fi

if [[ ! -d "$HUB_PRIVATE" ]]; then
  echo "error: hub-private not found at $HUB_PRIVATE"
  echo "clone it first, then re-run: just setup-private $DEVICE"
  exit 1
fi

HUB_PRIVATE="$(cd "$HUB_PRIVATE" && pwd)"
DEVICE_CONFIG="$HUB_PRIVATE/devices/$DEVICE.toml"

if [[ ! -f "$DEVICE_CONFIG" ]]; then
  echo "error: no config found for device '$DEVICE'"
  echo "expected: $DEVICE_CONFIG"
  echo ""
  echo "available devices:"
  ls "$HUB_PRIVATE/devices/" 2>/dev/null | sed 's/\.toml$//' | sed 's/^/  /' || echo "  (none)"
  exit 1
fi

link() {
  local target="$1"
  local link="$2"
  if [[ -L "$link" ]]; then
    echo "already linked: $link"
  elif [[ -e "$link" ]]; then
    echo "error: $link exists but is not a symlink — remove it and re-run"
    exit 1
  else
    ln -s "$target" "$link"
    echo "linked: $link -> $target"
  fi
}

link "$HUB_PRIVATE/clients/src"      "$HUB_ROOT/clients/src/private"
link "$HUB_PRIVATE/workflows/src"    "$HUB_ROOT/workflows/src/private"
link "$HUB_PRIVATE/.env"             "$HUB_ROOT/.env"
link "$DEVICE_CONFIG"                "$HUB_ROOT/hub.toml"

echo ""
echo "done. device: $DEVICE"
echo "edit hub-private/devices/$DEVICE.toml to configure integrations for this device."
echo "run 'just check' to verify compilation."
