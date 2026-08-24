# P6 — Fuzzing

**Status:** implemented  
**Serves:** G8  
**Must not:** require 10k seeds before the first swarm run; default-on fsync-lie; liveness flag on brutal partitions

**Done when:** `chronos-sim` runs N seeds headless, logs failures with seed + digest + check name. N=100 is enough to start; 10k is a stretch goal.

**Files:** `chronos-sim/src/{fuzz,main,trace}.rs`

---

## P6.A — Swarm config from seed

### P6.A.1

- **P6.A.1.a** Derive from RNG: `n ∈ {3,5}`, election range, heartbeat, delay ranges, drop/dup ppm, crash rate, partition rate, torn-suffix on, fsync-err on, **fsync-lie off**, buggify subset, client rate, `max_ns`.
- **P6.A.1.b** Mix: with probability ~1/2 use a “calm” profile (low drop, rare crash). FDB: too many faults collapse to “no leader.”
- **P6.A.1.c** Serialize config into the trace header so replay does not guess.

**Exit:** two runs of seed 7 produce the same config bytes and same digest.

---

## P6.B — Driver

### P6.B.1

- **P6.B.1.a** Loop: inject client ops, maybe crash/partition as scheduled events (not ad-hoc threads).
- **P6.B.1.b** Stop at `max_ns` or first failed check.
- **P6.B.1.c** On fail: write `fail-<seed>.trace` (or `fail-<seed>-<digest-prefix>.trace`), stdout: seed, digest, check, config summary.
- **P6.B.1.d** On success: optional compact coverage counters (which knobs fired).

### P6.B.2 CLI

- **P6.B.2.a** `chronos-sim --seed 42`
- **P6.B.2.b** `chronos-sim --seeds 100 --start 0` (seeds 0..99 or hashed)
- **P6.B.2.c** `--replay fail-42.trace` if you already record a full event list; otherwise `--seed 42` is enough.

**Exit:** `--seeds 20` finishes in reasonable time on one core.

---

## P6.C — CI-sized batch

### P6.C.1

- **P6.C.1.a** Document N for P9 (e.g. 50 on PR, 1000 nightly). Implement the flag now even if CI is P9.
- **P6.C.1.b** Determinism test still runs first (seed 42 twice).

**Exit:** local script `scripts/fuzz.sh 50` exists.

---

## Phase exit checklist

- [x] Headless swarm
- [x] Failure dump with seed
- [x] Calm + brutal mix
- [x] Fsync-lie still off
- [x] 10k seeds is a goal, not a blocker
