# Close G0 preferred path — seed 281 completion playbook

**Status:** CLOSED 2026-08-25 CLASS=CHECKER (no PROTOCOL pack)  
**Purpose:** Store every finding so far and the exact steps to finish the preferred G0 claim once.  
**Product v1:** already shipped via PLANTED pack. This file is only for the stronger “wild find” bar.

Do **not** enable `skip_vote_persist` in swarm / CLI minify. Do **not** retune election/drop to hide the fail.

---

## Definition of done (once and for all)

All of the following must be true:

1. [x] Live fail fixed: `--seed 281` CLEAN (same digest as the old fail file; oracle no longer misnames `Io`).
2. [x] Root cause classified: `CHECKER` (after HARNESS record/replay gap closed in Step 2).
3. [x] No PROTOCOL pack (N/A).
4. [x] Checker + heartbeat-book fixes in tree; `cargo test --workspace --release` green (Docker); re-swarm 1000 seeds.
5. [x] `--seeds 1000 --start 1 --out traces --coverage` exit 0, 0 CheckFails (2026-08-25 Docker).
6. [x] Outcome filled; hunt CLOSED; HANDOFF / deliverable still-open cleared.

PLANTED pack remains the shipped G9 harness proof.

---

## Results stored so far (do not lose)

### Shipped v1 (done)

| Item | Location / value |
|------|------------------|
| Public repo | https://github.com/sahilahmed21/Chronos |
| HEAD (2026-08-24 handoff) | `cd35c46260f8076725a2c2766f1d51c013972de8` |
| Planted pack COMMIT (green code pin) | `2b46ebb0c4216ead35a52352460a372e85ffae98` |
| Planted pack | `docs/bugs/2026-08-24-planted-skip-vote-persist/` CLASS=PLANTED |
| Planted check | ElectionSafety via `skip_vote_persist` (test-only hook) |
| CI | `ci / verify` green on planted COMMIT and HEAD |
| Local proof | Docker `rust:1-bookworm` `cargo test --workspace --release` green |
| Phases | P1–P9 shipped (G1–G11 tooling + PLANTED G9 fallback) |

### Honest swarm hunt (open)

| Item | Value |
|------|--------|
| Date | 2026-08-24 |
| Tree | `cd35c46` / CI-green equivalent |
| Host | Windows + Docker `rust:1-bookworm` |
| Command | `chronos-sim --seeds 1000 --start 1 --out traces --coverage` |
| Failures | **1 / 1000** |
| Seed | **281** |
| Check | **Linearizability** (`no linearization of the client history`) |
| Profile | brutal, n=3 |
| Digest | `cca2e7dcd3a609f5891e38bd9b0908c065d848cd6b70517c3d660acb14fae0e7` |
| `--seed 281` | FAIL (same digest) |
| `--replay traces/fail-281.trace` | `REPRODUCED` |
| `--minify --seed 281` | **`MINIFY MISMATCH`** (`HarnessMismatch`: full recorded schedule did **not** fail) |
| Artifacts (gitignored) | `traces/fail-281.trace`, `traces/fail-281.planned` |
| Hunt log | [HUNT-2026-08-24.md](HUNT-2026-08-24.md) |

### Coverage claim (same 1000-seed batch)

```text
# coverage runs=1000
profile: brutal 504, buggify 410, n3 507, n5 493
observed: crash 756, partition 736, drop 548, dup 386, fsync_err 334, torn 136
```

### What MINIFY MISMATCH means (locked interpretation)

- Live RNG path (`run_seed(281)`) fails Linearizability.
- Replaying **extras + ObservedSchedule book** with `DelayBind::Recorded` does **not** fail.
- Therefore something that affects the live fail is **not bound** by the recorded book (or recording/replay is wrong).
- Classification **so far:** HARNESS signal — **not** a finished PROTOCOL pack.
- `--replay` succeeding only proves the **fail file header** (seed/digest/check/config/extras) matches a fresh `run_seed`; it does **not** prove Recorded-book minify fidelity.

### Hard locks while closing this

- `Cluster::drain()` vs swarm/minify `drain_horizon()` — do not put horizon drain back on unit tests.
- `maybe_commit` → `replicate_all` when commit advances — keep.
- Planted `skip_vote_persist` is `#[cfg(test)]` only — never swarm/CLI minify.
- Do not invent COMMIT hashes; pin `git rev-parse` of a reproducing tree.

---

## Step-by-step (execute in order)

### Step 0 — Baseline (30 min)

Restore artifacts if missing; confirm the three CLI behaviors still match the table above.

```text
docker run --rm -v c:/projects/Chronos:/work -w /work -e RUST_TEST_THREADS=1 \
  rust:1-bookworm cargo test --workspace --release

docker run --rm -v c:/projects/Chronos:/work -w /work rust:1-bookworm \
  cargo run -p chronos-sim --release -- --seed 281 --out traces

docker run --rm -v c:/projects/Chronos:/work -w /work rust:1-bookworm \
  cargo run -p chronos-sim --release -- --replay traces/fail-281.trace

docker run --rm -v c:/projects/Chronos:/work -w /work rust:1-bookworm \
  cargo run -p chronos-sim --release -- --minify --seed 281
```

**Gate:** FAIL + same digest, REPRODUCED, MINIFY MISMATCH. If anything changed, update the “Results stored” table before continuing.

Optional: copy `traces/fail-281.trace` and `fail-281.planned` into `docs/bugs/_scratch-281/` (gitignored or temporary) so the hunt is not lost when `traces/` is wiped.

### Step 1 — Diff live RNG vs Recorded book (HARNESS) (half day)

**Goal:** name the unbound source of entropy / world choice.

Inspect (code, not vibes):

1. `ObservedSchedule` / `ReplayBook` fields in `crates/chronos-sim/src/cluster.rs` — what is recorded (delays, drops, dups, …).
2. `DelayBind::Recorded` vs `Defaults` in `minify.rs` `schedule_fails`.
3. What still calls the PRNG under Recorded: election timeouts, crash/partition timing, client inject, fsync_err / torn choices, buggify branches.
4. Whether swarm uses `drain_horizon` while Recorded replay differs in stop condition.
5. Whether extras in `fail-281.planned` are complete vs what `run_seed` actually injected.

**Method that works:**

- Add a temporary debug dump (or existing trace fields): for seed 281 live run, log every PRNG consumer tag + value.
- Re-run under Recorded with the same extras; compare the event stream / client history until the first divergence.
- The first divergence **is** the harness gap.

**Gate:** one written sentence: “Live and Recorded diverge at X because Y is not in the book.”

### Step 2 — Fix the gap (HARNESS) (half–1 day)

Correct underlying design (pick the real one):

- **Record** the missing choice into `ObservedSchedule` / extras and bind it under `DelayBind::Recorded`, **or**
- **Stop** sampling that choice on the live swarm path so live ≡ Recorded, **or**
- Fix a replay bug that ignores a recorded field.

Add a regression test: seed 281 (or a minimized unit that mocks the gap) so `schedule_fails(..., Recorded)` matches live check name.

**Gate:**

```text
chronos-sim --minify --seed 281
```

is **not** `MINIFY MISMATCH`. Expect either MINIFIED (exit 1) or a clear progress toward subset DD that still fails Linearizability.

### Step 3 — Minify + classify (half day)

```text
chronos-sim --minify --seed 281 --out traces
# → traces/fail-281.min
```

Read the minimized schedule. Decide CLASS:

| Evidence | CLASS | Action |
|----------|-------|--------|
| `step` violates an invariant under a legal schedule | PROTOCOL | Fix protocol; pack after fix proves seed clean |
| Oracle wrong (history / Raft check) | CHECKER | Fix checker; no PROTOCOL pack; re-swarm |
| Disk/net/sched lied | SIM | Fix sim; re-swarm |
| Still only recording/planner | HARNESS | Stay in Step 2 |

**Never** claim PROTOCOL until Recorded minify reproduces the check.

### Step 4A — If PROTOCOL: fix + pack

1. Fix root cause in `chronos-protocol`.
2. Prove: `--seed 281` CLEAN after fix; planted ElectionSafety tests still pass; `cargo test --workspace` green.
3. On the **pre-fix commit** (or a branch tip that still fails), keep failing digest + `schedule.min` for the pack.
4. Copy `_template/` → `docs/bugs/YYYY-MM-DD-linearizability-281/` (name honestly).
5. Fill SEED=`281`, COMMIT=`git rev-parse` of failing tree, `trace.sha256`, `schedule.min`, README, FIX.md, CLASS=`PROTOCOL`.
6. On mainline (fixed): FIX.md describes the patch; seed 281 no longer fails.

### Step 4B — If CHECKER / SIM / HARNESS only

1. Land the fix.
2. Re-run 1000-seed swarm.
3. If clean: document in Outcome below — preferred wild PROTOCOL not found; PLANTED remains G9. That still closes **this** open item (281).
4. If a **new** seed fails with minify that works: go to Step 3 for that seed.

### Step 5 — Nightly bar green (final)

```text
docker run --rm -v c:/projects/Chronos:/work -w /work rust:1-bookworm \
  cargo run -p chronos-sim --release -- --seeds 1000 --start 1 --out traces --coverage
```

**Gate:** exit 0, `0` CheckFails (or document intentional exceptions — there should be none for swarm).

Update:

- [x] This file → Outcome
- [x] `HUNT-2026-08-24.md` → CLOSED CLASS=CHECKER
- [x] `docs/HANDOFF.md` remaining table
- [x] `docs/roadmap/deliverable.md` / `goals.md` still-open bullets

---

## Suggested next-chat prompt (copy)

```text
Continue Chronos from docs/bugs/CLOSE-281.md and docs/HANDOFF.md.
Execute Step 0 then Step 1: find why DelayBind::Recorded full schedule is clean while run_seed(281) fails Linearizability.
Do not enable skip_vote_persist. Triage HARNESS → CHECKER → SIM → PROTOCOL.
Store findings in CLOSE-281.md Outcome / Working notes as you go.
```

---

## Working notes (fill during triage)

| Date | Note |
|------|------|
| 2026-08-24 | Hunt logged; MINIFY MISMATCH; CLASS pending HARNESS first. |
| 2026-08-24 23:09 | **Step 0 PASS (pwsh + Docker):** `--seed 281` FAIL same digest; `--replay` REPRODUCED; `--minify` MINIFY MISMATCH. Do not use Git Bash/`➜` shell for `-v c:/…:/work` (invalid mode). Stay on PowerShell. |
| 2026-08-24 23:47 | **Step 1 PASS (HARNESS gap named, no PROTOCOL claim):** Live and Recorded diverge at the first Heartbeat `TimerFired` (node 1, timer 1, gen 1: t=344669741 vs t=340743668) because heartbeat delay (`heartbeat_ns` + swarm `jitter()`) is not in `ReplayBook` (`send_delay` / `io_delay` / `election` only); Recorded forces `jitter()=0`. Brutal seed 281: `jitter_max_ns=5_000_000`. Events 0–24 matched; live Linearizability fail vs Recorded clean. |
| 2026-08-24 23:57 | **Step 2 PASS (HARNESS, no PROTOCOL claim):** Recorded `heartbeat` FIFO in `ReplayBook` (same `(node, life)` key as election). Swarm stores `heartbeat_ns + jitter()`; `DelayBind::Recorded` pops it; Defaults fall back to `heartbeat_ns`. Regression `seed_281_full_recorded_schedule_matches_live_check`. `--minify --seed 281` → `MINIFIED` Linearizability atoms 41→41 extras 29→29, wrote `traces/fail-281.min` (not MINIFY MISMATCH). Subset DD did not shrink under `DelayBind::Defaults`. |
| 2026-08-25 00:02 | **Step 3 CLASS=CHECKER (not PROTOCOL / SIM / HARNESS).** `fail-281.min` is the full extras (41→41); subset DD under Defaults did not shrink. Live history: client 2 req 4 `Put m=a` completes `Err(Io)` at t=3023525899, then `Get m` returns `Ok(a)` (client 2 req 7 t=3567218671; client 1 req 3 t=4989137125). Architecture § Black-box: unknown/timeout-after-possible-apply stays **incomplete**. Oracle treated `Io` as a definite miss (`is_definite_fail`), so a later Get of the applied value was a false Linearizability fail. `NotLeader` stays definite (no append). Not a Raft invariant break; not skip_vote_persist. Oracle fix: `Io` tries both apply and skip (same as incomplete). |
| 2026-08-25 00:12 | **Step 4B + 5 PASS.** No PROTOCOL pack. `--seed 281` ok; `--minify --seed 281` `MINIFY CLEAN`; Docker `cargo test --workspace --release` green; `--seeds 1000 --start 1 --out traces --coverage` exit 0, 0 CheckFails. Coverage: brutal 504, buggify 410, n3 507, n5 493; observed crash 756, partition 736, drop 548, dup 386, fsync_err 334, torn 136. PLANTED remains G9. |

## Outcome (fill when closed)

- **Closed date:** 2026-08-25
- **Final CLASS:** CHECKER (HARNESS gap for heartbeat delays closed first so Recorded minify could see the same history)
- **Root cause (one paragraph):** Seed 281’s client history had `Put m=a` complete `Err(Io)` after a possible apply, then later `Get m` returned `Ok(a)`. Architecture § Black-box requires timeout/unknown-after-possible-apply to stay incomplete. The linearizability oracle treated `Io` as a definite miss, so that history could not linearize. `NotLeader` remains a definite miss. Not a Raft safety break. Heartbeat jitter is now in `ReplayBook` so Recorded replay matches live (needed to classify; not the false fail).
- **Fix commit(s):** this commit (heartbeat FIFO in `ReplayBook`; `Io` searched as unknown in `history.rs`)
- **Pack path (or “none — harness/sim/checker only”):** none — harness/sim/checker only
- **`--seeds 1000` result:** exit 0, 0 CheckFails, no `FAIL` lines (Docker `rust:1-bookworm`, 2026-08-25, re-run). `--seed 281` CLEAN. `--minify --seed 281` `MINIFY CLEAN`.
- **G0 preferred status:** preferred wild PROTOCOL path abandoned; PLANTED pack remains G9; 281 closed as CHECKER + HARNESS record fix.
