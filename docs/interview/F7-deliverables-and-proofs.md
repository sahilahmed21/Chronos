# F7 — Deliverables achieved + proofs (interview evidence sheet)

**Purpose:** One place to answer *“What did you actually ship, and how do you prove it?”*  
**Rule:** Only claim what is checked below. Prefer citing a path, SHA, or Actions URL over adjectives.

**Related:** F1 (pitch), F5 (war stories), `docs/roadmap/deliverable.md`, `docs/roadmap/goals.md`.

---

## 0. One-line status (say this)

> Chronos v1 is **done** under `docs/roadmap/deliverable.md`: runnable Raft KV under a deterministic sim, safety + linearizability checkers, PLANTED ElectionSafety pack, minify/coverage/CI, and a closed hunt (seed 281 = CHECKER). We did **not** ship a wild PROTOCOL Raft-safety find or multi-node TCP HA.

---

## 1. v1 deliverable checklist (product DoD)

Source: `docs/roadmap/deliverable.md`. All boxes are **[x]** in-tree.

| # | Must-exist item | Status | Where / how you prove it in interview |
|---|-----------------|--------|----------------------------------------|
| 1 | Runnable crates + sim CLI | Done | `cargo test --workspace`; `cargo run -p chronos-sim -- --seed 7` |
| 2 | Determinism gates + Linux CI | Done | `.github/workflows/ci.yml` + `scripts/check-determinism-gates.sh` |
| 3 | Safety checkers + planted votedFor regressions | Done | `chronos-sim` checkers + `tests/properties.rs` planted tests |
| 4 | Bug pack linked | Done | `docs/bugs/2026-08-24-planted-skip-vote-persist/` CLASS=**PLANTED** |
| 5 | README product surface + non-goals | Done | Root `README.md` |
| 6 | `cargo test --workspace` on a linker machine | Done | Linux Actions; Docker `rust:1-bookworm` on Windows |
| 7 | Pack `COMMIT` filled | Done | Pack `COMMIT` = `2b46ebb0c4216ead35a52352460a372e85ffae98` |

### Stronger claims (beyond minimal v1)

| Item | Status | Proof |
|------|--------|-------|
| Seed 281 triage | Done | CLASS=**CHECKER** — `docs/bugs/HUNT-2026-08-24.md`, `docs/bugs/CLOSE-281.md` |
| Local Docker 1000-seed swarm | Done | Documented 2026-08-25 in CLOSE-281 / deliverable |
| GitHub 1000-seed swarm | Done | [Actions run 32767856965](https://github.com/sahilahmed21/Chronos/actions/runs/32767856965) — `workflow_dispatch` `swarm_seeds=1000` on `93880c5`; log `# coverage runs=1000` |
| Wild PROTOCOL pack | **Not found** | Honest non-claim; PLANTED remains G9 |

### Interview bar (can you do this live?)

1. Replay a seed / show digest story.  
2. Point at a short schedule (`schedule.min` in PLANTED pack) and name the invariant.  
3. State what the sim does **not** cover (membership, snapshots, Jepsen, fsync-lie-on, multi-node TCP HA).

---

## 2. Goals G0–G11 (what “done” means for each)

Source: `docs/roadmap/goals.md`. Status line: G1–G11 tooling + PLANTED G9 shipped; preferred wild PROTOCOL closed via CHECKER path on 281.

| Goal | Intent | Achieved? | Proof / artifact |
|------|--------|-----------|------------------|
| **G0** | Seed + digest + min schedule + RCA (wild preferred; PLANTED fallback allowed) | **Fallback path yes**; preferred wild PROTOCOL **no** | PLANTED pack = labeled near-miss/harness proof; hunt 281 = CHECKER fix |
| **G1** | Deterministic boundary (`step`, no host I/O/HashMap in protocol) | Yes | Crate graph; Clippy + CI grep |
| **G2** | Two interpreters, same `step` | Yes | `chronos-sim` + `chronos-node` both depend on protocol only |
| **G3** | Deterministic scheduler + digest | Yes | `(time, global seq)` heap; SHA-256 of encoded records |
| **G4** | Simulated network | Yes | delay / drop / dup / partition in sim |
| **G5** | Simulated disk | Yes | `bytes` vs `durable_len`; torn past durable; fsync-lie **off** |
| **G6** | Raft election + replication + KV | Yes | Static cluster; Get via log; persist-before-send/ack |
| **G7** | Executable safety | Yes | Five Raft safeties + persist checks + linearizability; planted hook regressions |
| **G8** | Schedule fuzzing | Yes | `--seeds` / swarm plan from seed |
| **G9** | Bug artifact | Yes (PLANTED) | Pack under `docs/bugs/…` |
| **G10** | Minimization tooling | Yes | `--minify`; PLANTED `schedule.min` is inject sequence (not CLI minify of a wild fail) |
| **G11** | Product quality | Yes | README, CI, coverage table |

**Say carefully about G0:**  
“G0’s *preferred* story is a wild PROTOCOL find. We shipped the *allowed* fallback: a production-class planted persist bug with a full pack, plus we closed a real swarm fail as a checker bug. I will not invent a wild PROTOCOL win.”

---

## 3. Phases P1–P9 (implementation surface)

| Phase | Theme | Shipped evidence |
|-------|--------|------------------|
| P1 | Foundation types / WAL / step boundary | `chronos-protocol` |
| P2 | Simulator kernel | scheduler, RNG, net, disk, cluster |
| P3 | Raft + KV | election, replication, machine |
| P4 | Faults | crash, partition, torn, drop/dup knobs |
| P5 | Properties | `check.rs`, `history.rs`, planted tests |
| P6 | Fuzz / swarm | `fuzz.rs`, CLI `--seeds` |
| P7 | Bug pack | PLANTED directory |
| P8 | Minify | `minify.rs`, CLI `--minify`, heartbeat in ReplayBook |
| P9 | Product | README, CI, `--coverage` |

---

## 4. Proofs you can point at (evidence catalog)

### 4.1 Determinism proof

| Proof | What it shows | How to cite |
|-------|---------------|-------------|
| Same seed → same digest | Trace is pure function of seed + code | CI determinism / double-run story; `chronos-sim --seed S` twice |
| SHA-256 of encoded records | No host paths/clocks in digest | Architecture D13; `trace` / digest code |
| No `HashMap` in protocol/sim | Iteration order cannot scramble traces | Clippy disallowed + `scripts/check-determinism-gates.sh` in CI |
| No `Instant` / `std::fs` / `std::net` in protocol | Host I/O cannot leak into `step` | Same gates + crate design |

**Live demo (PowerShell + Docker if no `link.exe`):**

```text
docker run --rm -v c:/projects/Chronos:/work -w /work rust:1-bookworm `
  cargo run -p chronos-sim --release -- --seed 7
```

Run twice; digests must match.

### 4.2 Safety proof (white-box + black-box)

| Proof | Check / property | Where |
|-------|------------------|--------|
| Election Safety | ≤1 leader per term | sim checkers |
| Leader Append-Only | Leader log doesn’t rewrite history | sim checkers |
| Log Matching | Matching index+term ⇒ matching prefix | sim checkers |
| Leader Completeness | Committed entries appear in later leaders | sim checkers |
| State Machine Safety | Same applied index ⇒ same command (+ idempotency table) | sim checkers |
| Persist-before-send | Vote / grant Send only after Meta durable | persist checker + planted regression |
| Persist-before-ack | Successful AE ack only after log durable | persist checker |
| Linearizability | Client history vs map spec | `history` / end-of-run |

**Planted regression commands:**

```text
cargo test -p chronos-sim --test properties planted_skip_vote_persist_fails_election_safety -- --exact
cargo test -p chronos-sim --test properties unplanted_solo_campaign_passes_persist_before_send -- --exact
```

(Exact test names as in pack README; if rename in tree, open `tests/properties.rs` and use the real names.)

### 4.3 Artifact proof (G9 PLANTED pack)

**Path:** `docs/bugs/2026-08-24-planted-skip-vote-persist/`

| File present in tree | Role |
|----------------------|------|
| `README.md` | Invariant, why unit test misses it, honesty label |
| `CLASS` | `PLANTED` |
| `COMMIT` | `2b46ebb0c4216ead35a52352460a372e85ffae98` (green code pin — **not** necessarily HEAD) |
| `trace.sha256` | Digest field for pack |
| `schedule.min` | Human schedule (inject sequence for planted test; **not** CLI minify of a wild fail) |
| `schedule.planned` | Planned inject dump |
| `FIX.md` | Near-miss / harness proof wording |

**Honesty line for interview:**  
“This pack proves the *oracles and harness* catch a production-class persist bug. Swarm never enables the hook. It is labeled PLANTED — not a wild find.”

### 4.4 CI / remote proof

| Proof | Event | Result | URL / pin |
|-------|-------|--------|-----------|
| Push verify (fmt, test, clippy, gates, swarm 32) | push `main` | success | e.g. [32768141181](https://github.com/sahilahmed21/Chronos/actions/runs/32768141181) on `55317a6` |
| Seed 281 fix landed | push | success | [32765115755](https://github.com/sahilahmed21/Chronos/actions/runs/32765115755) on `e6291f5` |
| **1000-seed remote swarm** | `workflow_dispatch` `swarm_seeds=1000` | success; `# coverage runs=1000` | [32767856965](https://github.com/sahilahmed21/Chronos/actions/runs/32767856965) on `93880c5` |
| Nightly cron | `0 6 * * *` UTC | schedule → 1000 seeds | `.github/workflows/ci.yml` |

**CI steps (what “verify” means):** format → `cargo test --workspace --release` → clippy protocol+sim `-D warnings` → determinism gates → swarm N seeds with `--coverage`.

### 4.5 Hunt / checker proof (seed 281)

| Fact | Value |
|------|--------|
| Hunt | 1000 seeds; 1 fail at seed **281** |
| Symptom | Linearizability |
| CLASS | **CHECKER** (not PROTOCOL) |
| Root cause | `ClientError::Io` treated as definite miss; Put may have applied |
| Fix | Search Io as unknown (apply **or** skip), same as incomplete |
| HARNESS sibling | Heartbeat delay FIFO in ReplayBook so Recorded minify matches live |
| After fix | `--seed 281` ok; `--minify` MINIFY CLEAN; 1000-seed green locally + on GitHub |

**Do not say:** “We found a Raft bug in the wild.”  
**Do say:** “Swarm found a false linearizability fail; we fixed the oracle and the recording gap.”

### 4.6 Engineering / harness proofs (non-PROTOCOL)

| Issue | Class | Proof of fix |
|-------|-------|--------------|
| Test drain burned heartbeat horizon / CI hang | HARNESS | `drain` vs `drain_horizon` split |
| Commit advanced but followers missed `leaderCommit` under idle drain | PROTOCOL-adjacent design lock | `maybe_commit` → `replicate_all` |
| Delayed-message scenario no leader | TEST/HARNESS | Election 2.0–2.5s > 800ms RTT; `wait_leader` |
| SHA-256 known-vector fail | HARNESS | Round constant corrected |
| Clippy `-D warnings` on stable | BUILD | `as_chunks` / loop unroll |

Details: F5.

---

## 5. What is explicitly **not** a deliverable (v1)

From architecture + deliverable — say these if asked “does it do X?”:

| Non-goal | Status |
|----------|--------|
| Dynamic membership | Out |
| Snapshots / compaction | Out |
| Read-index / lease reads | Out |
| Multi-node TCP HA cluster | Out — `chronos-node` is **single-node WAL demo** |
| Jepsen against real deploys | Out |
| Antithesis / Hermit of unmodified binary | Out |
| Fsync-lie default-on | Out (modeled, default **off**) |
| Byzantine nodes | Out |
| Wild PROTOCOL pack from swarm | **Not found** (not claimed) |

---

## 6. Pins to memorize (numbers / SHAs)

| Pin | Value | Meaning |
|-----|--------|---------|
| Planted pack COMMIT | `2b46ebb0c4216ead35a52352460a372e85ffae98` | Repro pin for PLANTED; do not casually re-pin to HEAD |
| 1000-seed GitHub SHA | `93880c530cbbc8c0887a8cdf2cd372430fd3808d` | SHA that ran remote 1000-seed dispatch |
| 281 fix SHA | `e6291f5c2c1f6957de84a64867ef496954b18be8` | Checker + heartbeat book |
| Public repo | https://github.com/sahilahmed21/Chronos | Evidence home |
| 1000-seed run | https://github.com/sahilahmed21/Chronos/actions/runs/32767856965 | Remote coverage proof |
| Spec | `docs/02-architecture.md` | Wins all conflicts |

*(If `main` moves, re-check Actions; planted COMMIT stays until you intentionally re-pin.)*

---

## 7. Spoken “deliverables + proofs” script (~90 seconds)

> Chronos v1 is complete against our written deliverable. We shipped three crates: a pure Raft KV `step` function, a deterministic simulator with swarm fuzz and minify, and a single-node real-WAL demo node.  
>  
> Proof of determinism: seeded runs, SHA-256 digests of encoded traces, and CI gates that ban HashMap and host I/O in the protocol.  
> Proof of safety: five Raft invariants plus persist-before-send/ack and linearizability, checked in the sim.  
> Proof of the harness: a PLANTED pack where skipping vote persist fails Election Safety — labeled PLANTED, hook off in swarm.  
> Proof of ops: Linux CI green on push, and a GitHub Actions run of one thousand seeds with coverage that exited clean.  
>  
> We also closed a thousand-seed hunt fail on seed 281 as a **checker** false positive — Io must be treated as unknown — not as a wild Raft PROTOCOL bug.  
> What we do not claim: multi-node TCP HA, Jepsen, Antithesis, or a wild protocol find we did not get.

---

## 8. Rapid-fire interviewer challenges

| Challenge | Answer |
|-----------|--------|
| “Show me the bug you found.” | PLANTED ElectionSafety pack + honesty label; and/or 281 as CHECKER writeup — not a fake PROTOCOL story. |
| “Is it production-ready HA?” | No. Correctness lab + single-node WAL demo. HA TCP is future work. |
| “How do I know CI isn’t theater?” | Point at Actions URLs; name steps; mention 1000-seed dispatch log line `# coverage runs=1000`. |
| “Same seed on Windows vs Linux?” | Digest excludes host fields; Docker/Linux CI is the gate; architecture requires cross-OS digest equality. |
| “Why isn’t G0 100%?” | Preferred wild PROTOCOL not found; allowed PLANTED fallback shipped; that is an honest G0. |

---

## 9. How this file fits the grilling pack

| File | Use with F7 |
|------|-------------|
| F1 | Pitch → when they ask “proof?”, open F7 §4 |
| F2 | Map files that implement each proof |
| F3 | Why these proofs are the right ones |
| F4 | Rust gates (Clippy bans) as part of determinism proof |
| F5 | Narrative behind 281 / drain / RTT |
| F6 | Drill Qs that cite these proofs |
| **F7 (this)** | Evidence sheet — deliverables + URLs + SHAs |

**Before every interview:** skim §1 checklist, §4 proofs, §5 non-goals, §6 pins.
