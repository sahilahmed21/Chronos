# P2 — Simulator

**Status:** implemented  
**Serves:** G3, G4.1, G5.3–G5.5  
**Must not:** Raft, Tokio, MadSim, linearizability oracle, swarm of 10k seeds

**Done when:** `hash(run(seed))` is identical on two consecutive runs; a unit test asserts it.

**Files:** `chronos-sim/src/{rng,scheduler,net,disk,cluster,trace,lib,main}.rs`

Use a **toy protocol** (ping/echo or the P1 single-node KV) so determinism is proven before Raft entropy.

---

## P2.A — PRNG

### P2.A.1

- **P2.A.1.a** Seeded RNG in `rng.rs`. ChaCha20 or Xoshiro. Integers only. New instance per run from `u64` seed. No process-global RNG.
- **P2.A.1.b** Helpers: `u64()`, `delay_ns(min, max)`, `bool(p_millionths)` — no `f64` probabilities (use integer parts per million).

**Exit:** two RNGs with seed 1 produce the same first 1000 `u64`s.

---

## P2.B — Scheduler (D4)

### P2.B.1 Queue

- **P2.B.1.a** World event enum: timer fire, message deliver, IoComplete, crash, partition, client inject, recover. (Crash/partition can be stubbed until P4 but the queue should accept them.)
- **P2.B.1.b** Min-heap keyed by `(Timestamp, Seq)`. `Seq` is **one** `u64` on the scheduler, `+= 1` at every enqueue. Never reset mid-run. Never per-node.
- **P2.B.1.c** Pop earliest; set `now = time`. No OS sleep. No threads.

### P2.B.2 Time

- **P2.B.2.a** Origin 0. Time only moves when an event is popped.
- **P2.B.2.b** When scheduling, add RNG jitter to the timestamp so ties are rare. `seq` still orders remaining ties. Do **not** collect a “ready this tick” list and shuffle it.

**Exit:** enqueue A at t=10 and B at t=10; order is determined by seq (enqueue order), test asserts that.

---

## P2.C — Sim disk (full model, faults off except delay=0)

### P2.C.1 Buffer

- **P2.C.1.a** Per node: `bytes`, `durable_len`, in-flight ops with completion times.
- **P2.C.1.b** `Append`: mutate `bytes` immediately in **emission order**. Do not move `durable_len`. Enqueue `IoComplete(Append)`.
- **P2.C.1.c** `Fsync`: snapshot `sync_len = bytes.len()` at submit. On Ok completion, `durable_len = max(durable_len, sync_len)`.

### P2.C.2 Completions

- **P2.C.2.a** Completion delay from RNG (may be 0 in tests).
- **P2.C.2.b** Append vs Fsync vs (later) Send completions may reorder. Two Appends to the same node’s `bytes` must not.

**Exit:** write then fsync; crash helper (even if P4 owns crash) that truncates to `durable_len` loses unsynced tail.

---

## P2.D — Sim net (happy path)

### P2.D.1

- **P2.D.1.a** On `Send`: assign `MsgId`, `delivery_at = now + delay_ns` (0 allowed), enqueue delivery.
- **P2.D.1.b** All-connected adjacency matrix. Drop/dup/partition functions exist but default off.
- **P2.D.1.c** In-flight messages: `BTreeMap<MsgId, ...>` or a `BTreeMap<(Timestamp, Seq), Msg>`.

**Exit:** ping from node 0 to 1 is delivered once in a 2-node toy run.

---

## P2.E — Cluster loop

### P2.E.1

- **P2.E.1.a** `cluster.rs`: own N nodes, disks, net, scheduler, RNG.
- **P2.E.1.b** Interpret effects in **emission order** from one `step`.
- **P2.E.1.c** `ArmTimer { kind }`: `at = now + duration(kind)` where Election duration is RNG in `[T, 2T]`, Heartbeat is fixed config. Enqueue `TimerFired`.
- **P2.E.1.d** Toy or P1 KV as the `step` body. Raft still off.

### P2.E.2 Trace

- **P2.E.2.a** After each world event: append a typed `TraceRecord` (time, seq, kind, node, ids). No hostname, path, `Instant`.
- **P2.E.2.b** SHA-256 of concatenation of little-endian encodings (`sha2` crate is allowed in **sim**, not protocol).
- **P2.E.2.c** `run(seed, config) -> digest`.

### P2.E.3 Determinism test (G3.3)

- **P2.E.3.a** `assert_eq!(run(42), run(42))`.
- **P2.E.3.b** Run twice in one process with fresh cluster each time (no leftover `bytes`).
- **P2.E.3.c** Optional: `run(42) != run(43)` so the hash is not accidentally constant.

**Exit:** that test is the phase gate. `chronos-sim` binary can print digest for seed 42.

---

## Phase exit checklist

- [x] Global seq, not per-node
- [x] SHA-256 traces, no `std::hash`
- [x] Same seed twice → same digest
- [x] Still no Raft *(at P2 exit; Raft added in P3)*
- [x] HashMap still absent from protocol and sim
