# Finding: \<invariant\>

- Seed: see `SEED`
- Commit: see `COMMIT`
- Trace SHA-256: see `trace.sha256`
- Class: see `CLASS`
- Replay: `chronos-sim --seed $(cat SEED)` and `chronos-sim --replay fail.trace`
- Minify: `chronos-sim --minify --seed $(cat SEED)` then copy `fail-<seed>.min` to `schedule.min`

## What broke

Which check. Which nodes. Terms and log indices. Snapshots from the fail-file `detail` line.

## Schedule

See `schedule.planned` (P7, full swarm extras) and `schedule.min` (P8, minimized atoms). Minify success is the same check name, not the original digest.

## Why a unit test would miss it

Needs a multi-node schedule (partition, crash-before-fsync, reorder), not a single `handle_request_vote` case.

## Root cause

## Fix or limitation

See `FIX.md`.
