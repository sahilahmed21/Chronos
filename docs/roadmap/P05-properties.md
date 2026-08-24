# P5 — Property checking

**Status:** implemented  
**Serves:** G7  
**Must not:** skip the planted-bug proof; mix liveness into default safety runs

**Done when:** (1) planted missing `votedFor` persist fails Election Safety; (2) unplanted happy path + P4 scenarios pass all safety checks.

**Files:** `chronos-sim/src/{check,history}.rs`, optional `chronos-protocol` test hook / feature `plant-skip-vote-persist`

---

## P5.A — White-box Raft (after every world event)

### P5.A.1 Five properties

- **P5.A.1.a** Election Safety: `BTreeMap<Term, NodeId>` of leaders observed; at most one.
- **P5.A.1.b** Leader Append-Only: snapshot each leader’s log for this term; never shrinks/overwrites.
- **P5.A.1.c** Log Matching: if two logs share (index, term), prefixes equal.
- **P5.A.1.d** Leader Completeness: when a leader of term T advances commitIndex, record `committed[index]=entry`; later-term leaders must contain it.
- **P5.A.1.e** State Machine Safety: `applied[index]` unique, including idempotency table as part of applied KV state.

### P5.A.2 Engineering invariants

- **P5.A.2.a** Persist-before-send: outgoing RequestVote / grant vs `durable` cookie.
- **P5.A.2.b** Persist-before-ack: AE success vs durable last_index.
- **P5.A.2.c** `commitIndex` never decreases.
- **P5.A.2.d** Commit by replica count only if `entry.term == currentTerm`.
- **P5.A.2.e** `matchIndex[self]` waits for fsync Ok.

On fail: halt, dump seed, digest, check name, node snapshots.

**Exit:** checks run on P3 happy path without firing.

---

## P5.B — Planted bug (checker proof)

### P5.B.1

- **P5.B.1.a** Feature or test-only hook: send RequestVote **before** Meta fsync (sofa-jraft class).
- **P5.B.1.b** Scenario: vote, crash, recover, vote other candidate, same term.
- **P5.B.1.c** Assert Election Safety (or persist-before-send) **fails** with the hook on, **passes** with hook off.

**Exit:** this test is sacred; do not delete after P7.

---

## P5.C — Black-box linearizability (G7.3)

### P5.C.1 History

- **P5.C.1.a** Simulated clients: invoke at `now`, complete on `Reply` or timeout (incomplete).
- **P5.C.1.b** Same `(client, request)` on retry.

### P5.C.2 Checker

- **P5.C.2.a** Sequential spec = a `BTreeMap` key→value. Porcupine-style search is OK at small history sizes; bound ops per run.
- **P5.C.2.b** Do **not** compare replica maps every event.
- **P5.C.2.c** Test: concurrent Put/Get from 2 clients, no faults, history linearizable.
- **P5.C.2.d** Counter-test (optional): if someone implements Get from leader memory, checker should fail under stale leader — do not implement that Get.

**Exit:** happy-path history passes; incomplete ops don’t crash the checker.

---

## P5.D — Liveness (separate)

### P5.D.1

- **P5.D.1.a** Flag `check_liveness`. Predicate: majority fully connected for `10 * max_election_timeout`, no crashes → a leader exists and a write commits.
- **P5.D.1.b** Default **off** in partition scenarios. One dedicated test with the flag on and a healthy cluster.

**Exit:** liveness test passes; partition tests do not use the flag.

---

## Phase exit checklist

- [x] All five safety properties
- [x] Persist-before-send/ack
- [x] Planted votedFor bug fails
- [x] Linearizability on client history
- [x] Liveness isolated
