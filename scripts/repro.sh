#!/usr/bin/env bash
# P7: run one swarm seed twice. Exit 2 if digest or check lines differ.
# Usage: scripts/repro.sh <seed>
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
if [[ $# -lt 1 ]]; then
  echo "usage: scripts/repro.sh <seed>" >&2
  exit 2
fi
seed="$1"

seed_line() {
  local out line
  out="$(cargo run -q -p chronos-sim -- --seed "$1" 2>&1 || true)"
  line="$(printf '%s\n' "$out" | grep -E '^(ok|FAIL|ABORT) seed=' | tail -n 1 || true)"
  if [[ -z "$line" ]]; then
    printf '%s\n' "$out" >&2
    echo "no swarm status line for seed $1" >&2
    exit 2
  fi
  printf '%s\n' "$line"
}

a="$(seed_line "$seed")"
b="$(seed_line "$seed")"
printf '%s\n' "$a"
printf '%s\n' "$b"
if [[ "$a" != "$b" ]]; then
  echo "MISMATCH seed $seed" >&2
  exit 2
fi
if [[ "$a" == ok\ * ]]; then
  exit 0
fi
exit 1
