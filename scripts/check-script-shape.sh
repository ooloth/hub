#!/usr/bin/env bash
# Fails if a Python script in scripts/ is not a uv single-file script.
#
# A script declaring `python3` runs under whatever that resolves to on the
# caller's PATH, and says nothing about what it needs, so adding one dependency
# means adding a venv and an install step that only the author knows about. A uv
# single-file script carries its interpreter constraint and its dependencies
# inline and runs from a fresh clone.
#
# The shape is the whole rule: one file per script, nothing imported between
# them. A script that needs a second file is an application. See the Scripts
# section of ~/.agents/standards/python.md
set -euo pipefail

cd "$(dirname "$0")/.."

SHEBANG='#!/usr/bin/env -S uv run --script'
failed=0

for script in scripts/*.py; do
  [[ -e "$script" ]] || continue

  if [[ "$(head -n 1 "$script")" != "$SHEBANG" ]]; then
    echo "$script: first line must be exactly"
    echo "  $SHEBANG"
    failed=1
  fi

  # PEP 723 metadata, which is what lets uv resolve dependencies without a venv.
  if ! head -n 10 "$script" | grep -q '^# /// script$'; then
    echo "$script: missing a PEP 723 block in its first 10 lines, e.g."
    echo "  # /// script"
    echo "  # requires-python = \">=3.12\""
    echo "  # dependencies = []"
    echo "  # ///"
    failed=1
  fi

  if [[ ! -x "$script" ]]; then
    echo "$script: not executable, so its shebang never runs (chmod +x)"
    failed=1
  fi
done

if [[ "$failed" -ne 0 ]]; then
  echo
  echo "Scripts are single-file uv scripts. See the Scripts section of"
  echo "~/.agents/standards/python.md"
  exit 1
fi
