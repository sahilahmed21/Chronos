#!/usr/bin/env bash
# D6 / D16: protocol and sim source must not use HashMap/HashSet or host I/O types.
# Comment lines are ignored. Clippy does not catch `use std::fs`.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
fail=0

check() {
  local dir="$1"
  local pattern="$2"
  local hits
  hits="$(grep -R --include='*.rs' -n -E "$pattern" "$dir" | grep -v -E ':[0-9]+:[[:space:]]*(//|//!|///)' || true)"
  if [ -n "$hits" ]; then
    echo "$hits"
    fail=1
  fi
}

for dir in "$root/crates/chronos-protocol/src" "$root/crates/chronos-sim/src"; do
  check "$dir" 'std::collections::HashMap|std::collections::HashSet'
done
check "$root/crates/chronos-protocol/src" 'std::fs|std::net::|std::thread::|std::time::Instant|std::time::SystemTime'

if [ "$fail" -ne 0 ]; then
  echo "determinism gate failed"
  exit 1
fi
echo "determinism gates ok"
