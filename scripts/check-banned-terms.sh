#!/usr/bin/env bash
set -euo pipefail

# Blocks commits containing terms that must not appear in the public hub repo.
# The term list lives in hub-private so the terms themselves stay private.
# Silently passes if hub-private is not present.

# Anchor the blocklist path to the repo root, not the caller's CWD. Git hook
# runners (e.g. prek's `hook-impl`) may invoke this script from a directory
# where the relative `../hub-private` path does not resolve; without anchoring,
# the `-f` guard below would silently pass and let banned terms through.
ROOT="$(git rev-parse --show-toplevel)"
BLOCKLIST="$ROOT/../hub-private/scripts/blocked-terms.txt"
[[ -f "$BLOCKLIST" ]] || exit 0

staged=$(git diff --cached --name-only --diff-filter=d)
[[ -z "$staged" ]] && exit 0

# Read staged content from the git index (not from disk) so that symlinks to
# private files are stored as their target path string, not followed to their
# content. This avoids false positives when a new symlink into hub-private is
# staged — git stores only the path, which contains no banned terms.
if echo "$staged" | while IFS= read -r f; do git show ":$f" 2>/dev/null; done \
   | rg --quiet -iF --file="$BLOCKLIST" 2>/dev/null; then
  echo "error: staged files contain a term banned from the public hub repo"
  echo "  see ../hub-private/scripts/blocked-terms.txt for the list"
  echo "  move the content to hub-private or use a generic identifier"
  exit 1
fi
