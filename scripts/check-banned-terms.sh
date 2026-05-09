#!/usr/bin/env bash
set -euo pipefail

# Blocks commits containing terms that must not appear in the public hub repo.
# The term list lives in hub-private so the terms themselves stay private.
# Silently passes if hub-private is not present.

BLOCKLIST="../hub-private/scripts/blocked-terms.txt"
[[ -f "$BLOCKLIST" ]] || exit 0

staged=$(git diff --cached --name-only --diff-filter=d)
[[ -z "$staged" ]] && exit 0

if echo "$staged" | tr '\n' '\0' | xargs -0 rg --quiet -iF --file="$BLOCKLIST" 2>/dev/null; then
  echo "error: staged files contain a term banned from the public hub repo"
  echo "  see ../hub-private/scripts/blocked-terms.txt for the list"
  echo "  move the content to hub-private or use a generic identifier"
  exit 1
fi
