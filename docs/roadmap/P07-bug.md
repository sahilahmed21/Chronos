# P7 — Bug discovery

**Status:** PLANTED pack shipped (harness proof). Hunt seed 281 closed CLASS=CHECKER ([../bugs/HUNT-2026-08-24.md](../bugs/HUNT-2026-08-24.md)). No wild PROTOCOL pack.  
**Serves:** G9  
**Must not:** “fix” paper-Raft-vs-fsync-lie and call it a DST win; silently retune swarm until it goes green without a writeup; enable `skip_vote_persist` to force a minify

**Done when:** `docs/bugs/<name>/` exists with seed, commit, digest, explanation — real find **or** labeled near-miss.

**Files:** `docs/bugs/`, `chronos-sim` `--replay` verifier, `scripts/repro.{ps1,sh}`

---

## P7.A — Hunt

### P7.A.1

- **P7.A.1.a** Run larger local batches (thousands) with mixed profiles.
- **P7.A.1.b** Triage each fail: checker bug vs sim bug vs protocol bug. Fix checker/sim first; those are not the portfolio artifact.
- **P7.A.1.c** If protocol: keep the seed sacred. Do not “just bump timeouts” to hide it.

---

## P7.B — Reproduce

### P7.B.1

- **P7.B.1.a** Clean tree, `chronos-sim --seed S` twice → same digest, same check. `chronos-sim --replay FILE` prints `REPRODUCED` / `CLEAN` / `DID_NOT_REPRODUCE` / `MISMATCH` after verifying digest, check, config, and extras (exit 0 / 1 / 2).
- **P7.B.1.b** Record COMMIT hash. Note if trace depends on git dirty state (it must not). Requires a git repository.

---

## P7.C — Explain

### P7.C.1 Writeup

- **P7.C.1.a** Which invariant. Which nodes. Terms and log indices.
- **P7.C.1.b** Fault sequence in English (partition, crash-before-fsync, reorder). `schedule.planned` in P7; `schedule.min` is P8.
- **P7.C.1.c** Why a unit test of `handle_request_vote` would not have seen it (needs a schedule).
- **P7.C.1.d** If it is a known Raft limitation (D10), label **near-miss** and cite PAR. Do not patch Raft to “survive lying fsync” in v1.

### P7.C.2 Fix or not

- **P7.C.2.a** If real protocol bug: patch, same seed passes, add regression test that replays the seed (full or minimized in P8).
- **P7.C.2.b** If sim lied (wrong disk model): fix sim, do not claim a Raft bug.

---

## P7.D — Fallback (near-miss)

If swarm is clean:

- **P7.D.1.a** Use P5 planted persist-after-send (or old-term commit, or Get-from-memory) as the artifact.
- **P7.D.1.b** README says: not found in random swarm; planted to prove the harness. Still include seed + trace + why unit tests miss it.

**Exit:** `docs/bugs/` matches [deliverable.md](deliverable.md) section 4 except `schedule.min` (P8). P7 ships `schedule.planned`.

---

## Phase exit checklist

- [x] `--replay` verifies digest, check, config, extras (`fail_file_header` / `verify_replay`)
- [x] Pack template and CLASS taxonomy in `docs/bugs/`
- [x] Seed replays on a linked binary (Linux CI + Docker `rust:1-bookworm`)
- [x] Honest hunt run + writeup for PLANTED fallback ([../bugs/2026-08-24-planted-skip-vote-persist/](../bugs/2026-08-24-planted-skip-vote-persist/))
- [x] Hunt log for seed 281 — CLOSED CLASS=CHECKER ([../bugs/HUNT-2026-08-24.md](../bugs/HUNT-2026-08-24.md))
- [x] Seed 281 triaged: HARNESS heartbeat book + CHECKER `Io`-as-unknown; no PROTOCOL pack
