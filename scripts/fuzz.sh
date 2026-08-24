#!/usr/bin/env bash
# P6 swarm batch. Usage: scripts/fuzz.sh [N]
# P9: N=50 on PR, N=1000 nightly. 10k is a stretch goal, not a start gate.
# P7 hunt starts at N=100.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
n="${1:-100}"
cd "$root"
mkdir -p "$root/traces"
exec cargo run -p chronos-sim -- --seeds "$n" --start 0 --out "$root/traces"
