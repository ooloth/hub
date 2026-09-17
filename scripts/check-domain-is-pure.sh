#!/usr/bin/env bash
# Fails if anything in domain/ reads ambient state.
#
# domain/ computes only from its arguments, so a domain type is built the same
# way in a test, in the TUI and in a workflow. The compiler already stops it
# importing another hub crate; nothing stops it reading a clock or a file, which
# is what this covers. See docs/invariants/domain-is-pure.md
set -euo pipefail

cd "$(dirname "$0")/.."

PATTERN='std::env|std::fs|std::process|SystemTime|Instant::now|Utc::now|Local::now|rand::|thread_rng'

if matches=$(rg --line-number --glob '*.rs' "$PATTERN" domain/src/ 2>/dev/null); then
  echo "domain/ must not read ambient state, but does:"
  echo "$matches"
  echo
  echo "A domain type that reads the clock, the environment, the filesystem or a"
  echo "random source is no longer a pure function of its inputs. See"
  echo "docs/invariants/domain-is-pure.md"
  exit 1
fi
