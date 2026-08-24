# P8 — Minimization

**Status:** tooling implemented and exercised in Linux CI. PLANTED pack has an inject-sequence `schedule.min` (not CLI minify). Heartbeat delays are in `ReplayBook` so Recorded minify matches live. Hunt 281 closed CHECKER (`MINIFY CLEAN` after oracle fix).  
**Serves:** G10  
**Must not:** delete Raft internals at random; rely on PRNG to recreate dropped faults; enable `skip_vote_persist` from the CLI

**Done when:** a long failing trace shrinks to the smallest **schedule** that still fails the same check (or a documented close subset).

**Files:** `chronos-sim/src/minify.rs`, `docs/bugs/<name>/schedule.min`

---

## P8.A — Record the schedule

### P8.A.1

- **P8.A.1.a** Extra decisions are atoms: crash/partition windows, client ops, fail-next-fsync, drop/dup **content tokens** `(from, to, hash(encoded Message))`. Torn length is pinned. Not `MsgId` / `IoId` ordinals.
- **P8.A.1.b** Happy-path protocol messages still happen via `step`; you do **not** delta-debug those away except by removing the fault that caused them.
- **P8.A.1.c** Replay mode: `run_schedule` (ppm 0, world `Rng` unread, `ReplayBook` delays + remaining tokens). Not “same RNG after deleting an event.”

**Exit:** unfiltered schedule still fails the same `CheckName` as the swarm drain (digest may differ).

---

## P8.B — Delta-debug

### P8.B.1

- **P8.B.1.a** Remove halves of the atom list; keep a subset if the **same check** still fails. Aborts never count.
- **P8.B.1.b** Then 1-by-1 removal until fixpoint. Cap candidate drains (10k) stops **proposing**; it is not `HarnessMismatch`. Full-schedule verify always drains (`DelayBind::Recorded`). Shrunk min verifies with `DelayBind::Defaults`.
- **P8.B.1.c** Optional: binary-search duration of a partition (not implemented; not a start gate).

### P8.B.2 Output

- **P8.B.2.a** `chronos-sim --minify --seed S [--out dir]` writes `fail-S.min` (human `schedule.min` text).
- **P8.B.2.b** Re-run min schedule → same check. Count atoms/extras before/after.

**Exit:** pack `schedule.min` when a P7 CLASS exists.

---

## P8.C — Regression

### P8.C.1

- **P8.C.1.a** Tests: unpaired heal is a heal; same-time extras keep orig order; search cap ≠ mismatch; DD keeps only the causal atom; full observed schedule matches swarm `CheckName`; `minify(1)` / public `minify_input` never plant `skip_vote_persist`. Planted hook stays `#[cfg(test)]`.
- **P8.C.1.b** If the bug was fixed in P7, minify the **pre-fix** commit or keep a `tests/regressions/seed_*` that uses a feature hook.

**Exit:** tool documented (`chronos-sim --minify --seed S`).

---

## Phase exit checklist

- [x] Schedule, not “delete Raft messages blindly”
- [x] Replay without hoping the RNG lines up
- [x] Minimized artifact path exists for PLANTED pack (`docs/bugs/2026-08-24-planted-skip-vote-persist/schedule.min` = inject sequence)
- [ ] Wild PROTOCOL pack `schedule.min` from `chronos-sim --minify` (optional; only after a classified swarm fail)
