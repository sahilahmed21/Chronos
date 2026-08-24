# P4 — Fault injection

**Status:** implemented  
**Serves:** G4.2–G4.3, G5.3–G5.5  
**Must not:** turn fsync-lie **on** in default scenarios; Byzantine; “whatever the RNG does” as the only tests

**Done when:** you can name a fault schedule in a test and **predict** Raft’s reaction, then watch the sim match the prediction.

**Files:** `chronos-sim/src/{net,disk,cluster,fuzz}.rs` (fuzz config types only), tests under `chronos-sim/tests/scenarios/`

---

## P4.A — Network faults

### P4.A.1 Knobs

- **P4.A.1.a** Per-link or global: delay range, drop probability (integer ppm), duplicate probability.
- **P4.A.1.b** Partition: set adjacency (symmetric default). Asymmetric = a flag, one matrix still.
- **P4.A.1.c** Record each drop/dup/partition on the trace (needed for P8).

### P4.A.2 Scripted scenarios

- **P4.A.2.a** **Partition leader from majority** at `t`, hold `D_ns`, heal. Predict: majority elects new leader; old leader steps down on heal; no two leaders in same term; uncommitted minority entries may vanish.
- **P4.A.2.b** **Message delay** large enough to trigger election timeout. Predict: election, then progress.
- **P4.A.2.c** **Drop all votes** for one candidate. Predict: split or retry; safety holds.
- **P4.A.2.d** Messages in flight **survive sender crash**; disk completions of crashed node do not.

**Exit:** each scenario is a named test with comments stating the prediction.

---

## P4.B — Disk faults (honest)

### P4.B.1 Crash

- **P4.B.1.a** Keep `bytes[0..durable_len]`. Build torn suffix from in-flight Appends (0..=len, seed-chosen). Mid-record OK.
- **P4.B.1.b** Drop in-flight I/O. Do not deliver their IoComplete. Bump incarnation on Recover.
- **P4.B.1.c** Recover: CRC scan, truncate, reload Meta/log, follower, ArmTimer Election.

### P4.B.2 Fsync Err

- **P4.B.2.a** Complete Fsync with Err; `durable_len` unchanged.
- **P4.B.2.b** Scenario: fail fsync of vote Meta → **no RequestVote on wire**.

### P4.B.3 Torn suffix (not D10)

- **P4.B.3.a** Crash during Append of an Entry; recover; last record CRC-fails; prefix intact.
- **P4.B.3.b** Assert this does **not** shrink `durable_len` below last successful fsync.

### P4.B.4 Fsync-lie knob

- **P4.B.4.a** Implement `fsync_ok_but_not_durable` **off** by default. A test may turn it on to show Raft breaks (document, do not add to default fuzz). That test is characterization, not a “bug we will fix in v1.”

**Exit:** crash/restart cluster still elects; planted persist-before-send crash matches G9-class story.

---

## P4.C — Process faults

### P4.C.1

- **P4.C.1.a** Crash any node by id at time t; restart after delay.
- **P4.C.1.b** Crash leader; predict new election; committed entries survive (majority durable).

**Exit:** committed Put still readable after leader crash+restart.

---

## P4.D — Buggify (legal weirdness)

### P4.D.1

- **P4.D.1.a** Extra delay on fsync; extra reject of an AE that could succeed; election timeout at range edge.
- **P4.D.1.b** Flags stored in run config / trace. Off in P3 happy-path tests.

**Exit:** one test with buggify on still preserves safety (or fails a checker for a known reason).

---

## Phase exit checklist

- [ ] Named scenario tests with written predictions
- [ ] Torn suffix ≠ fsync-lie
- [ ] Default fuzz knobs: delay, crash, torn suffix, fsync Err, partition — lie off
- [ ] Checkers not required yet, but nothing obviously two-leaders-same-term in these scripts (manual or debug print OK until P5)
