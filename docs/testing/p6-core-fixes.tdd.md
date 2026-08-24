# TDD evidence: P6 core-review fixes

## Source plan

Journeys were derived from the P6 core review (NEEDS FIX / NOT READY), not from a `*.plan.md` file. Locked P6 architecture was not redesigned.

This workspace is not a git repository (`git rev-parse` fails). Checkpoint commits were skipped.

## User journeys

1. As a fuzzer, I want every planned heal and restart at `t ≤ max_ns`, so `Cluster::drain` actually applies the pair.
2. As a fuzzer, I want crash windows on a node to be disjoint, so Recover is not applied to a live node.
3. As a replica, I want `fail_next_fsync` cleared on crash, so the next incarnation is not poisoned by the previous life.
4. As an operator, I want coverage counters to mean “this fault fired,” not “this knob was configured.”
5. As an operator, I want the CLI to reject `--seeds 0` and `--start + --seeds` that overflow `u64`.

## Task report

### Horizon-aware paired extras

- **Summary:** Planner picks delay/hold first, then a start time so `start + follow_up ≤ max_ns`. Occupancy for crashes uses the full `[at, recover_at)` window.
- **Validation:** `cargo check --workspace --tests --bins` (compile). Runtime tests not linked.
- **RED:** `cannot find function timestamps_within` / `crash_windows_disjoint` in `crates/chronos-sim/src/fuzz.rs` (E0425).
- **GREEN (compile):** same command, `Finished dev profile` after helpers + planner change.
- **Guarantee if tests run:** seeds 0..63 have every extra `t ≤ max_ns`; a heal at 7e9 with `max_ns = 5e9` is rejected by the helper.

### Disjoint crash windows + Recover-while-up

- **Summary:** Planner refuses overlapping down intervals. Apply-time: Crash is a ghost if the node is already down; Recover is a ghost if the node is up and `life > 0` (bootstrap Recover at `life == 0` still applies).
- **Validation:** compile as above.
- **RED:** tests referenced `crash_windows_disjoint` before it existed.
- **GREEN (compile):** helpers + occupancy + `is_ghost` arms.
- **Guarantee if tests run:** overlapping `[50,400)` and `[100,350)` on node 0 is rejected; adjacent `[50,100)` / `[100,150)` is accepted.

### `fail_next_fsync` incarnation-scoped

- **Summary:** `SimDisk::tear` clears `fail_next_fsync`.
- **Validation:** compile. Runtime test `crash_clears_fail_next_fsync` was not executed.
- **RED path:** compile-time for other tests; this test would be runtime RED on the old `tear` (assert `!fail_next_fsync` after `crash()`).
- **GREEN (compile):** `tear` now sets `fail_next_fsync = false`.

### Coverage means fired

- **Summary:** `Cluster` counts `dup_sends` on the duplicate-send branch and `torn_applied` when resolved `torn_extra > 0`. `run_seed` copies those observations.
- **Validation:** compile.
- **RED:** `no method named dup_sends` / `torn_applied` (E0599) in `cluster.rs` tests.
- **GREEN (compile):** getters + increments exist.

### CLI range parse

- **Summary:** `--seeds 0` and `start.checked_add(count - 1) == None` are errors.
- **Validation:** compile. Runtime parse tests not executed.
- **RED path:** would fail at runtime until parse was fixed (`--seeds 0` previously produced an empty batch).
- **GREEN (compile):** parse rejects those cases.

### encode_extra closed set

- **Summary:** unplanned `WorldEvent` kinds are skipped; no `255` poison tag.
- **Validation:** compile. `encode_extras_skips_unplanned_kinds` not executed.

## Test specification

| # | What is guaranteed | Test file or command | Test type | Result | Evidence |
|---|--------------------|----------------------|-----------|--------|----------|
| 1 | Every planned extra for seeds 0..63 has `t ≤ max_ns` | `fuzz.rs`: `swarm_never_enables_lie_or_liveness` | unit | UNRUN | `cargo test --workspace --offline` → `linker link.exe not found` |
| 2 | A heal past the horizon fails `timestamps_within` | `fuzz.rs`: `heal_past_horizon_is_rejected_by_the_invariant` | unit | UNRUN | same linker failure |
| 3 | Overlapping crash windows fail `crash_windows_disjoint` | `fuzz.rs`: `overlapping_crash_windows_are_rejected_by_the_invariant` | unit | UNRUN | same |
| 4 | Adjacent `[from, until)` windows on one node are disjoint | `fuzz.rs`: `adjacent_crash_windows_are_disjoint` | unit | UNRUN | same |
| 5 | Crash clears `fail_next_fsync` | `disk.rs`: `crash_clears_fail_next_fsync` | unit | UNRUN | same |
| 6 | Configured dup/torn without a send/crash do not count as fired | `cluster.rs`: `unused_dup_and_torn_counters_start_unfired`; `fuzz.rs`: `coverage_is_observed_not_configured` | unit | UNRUN | same |
| 7 | Specified `torn_extra > 0` sets `torn_applied` | `cluster.rs`: `specified_torn_extra_marks_torn_applied` | unit | UNRUN | same |
| 8 | `--seeds 0` and overflowing `--start`+`--seeds` are parse errors | `main.rs`: `parse_rejects_zero_seeds`, `parse_rejects_seed_range_overflow` | unit | UNRUN | same |
| 9 | Unplanned extras are omitted from the extras encoding | `fuzz.rs`: `encode_extras_skips_unplanned_kinds` | unit | UNRUN | same |
| 10 | Named seed 7 is a clean checker run | `fuzz.rs`: `run_seed_7_is_clean` | integration | UNRUN | same |
| 11 | Tests compile against the new helpers and getters | `cargo check --workspace --tests --bins` | compile | PASS | `Finished dev profile` after the fix |
| 12 | Clippy deny-warnings | `cargo clippy --workspace --all-targets -- -D warnings` | lint | PASS | `Finished dev profile` |
| 13 | No HashMap/HashSet in protocol/sim | `scripts/check-determinism-gates.ps1` | gate | PASS | `determinism gates ok` |

## Coverage and known gaps

- Coverage command not run. `cargo tarpaulin` / `llvm-cov` are not part of this workspace, and `cargo test` cannot link on this machine (no MSVC `link.exe`).
- Do not treat clippy or `cargo check` as G8. G8 still requires two linked runs of the same seed producing the same digest.
- Intentional: bootstrap `Recover` at `life == 0` is not a ghost, even if `alive` starts true.
- Not claimed: any specific seed’s `check.is_none()` at runtime.

## Merge evidence

No git commits. RED/GREEN summary:

- **RED:** `cargo check --workspace --tests --bins` failed with E0425 (`timestamps_within`, `crash_windows_disjoint`) then E0599 (`dup_sends`, `torn_applied`).
- **GREEN (compile):** same check command succeeded; clippy `-D warnings` succeeded; determinism grep succeeded.
- **Not green:** `cargo test --workspace --offline` failed with `linker link.exe not found`.
