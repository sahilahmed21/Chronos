# Goals

The product goal is not “implement Raft.” It is **architect away nondeterminism**, then use that to produce a replayable correctness artifact.

## Status (2026-08-25)

G1–G11 tooling and the PLANTED G9 fallback are **shipped**. G0’s preferred wild PROTOCOL path was closed as **CHECKER** on hunt seed 281 (false Linearizability: `Io` treated as a definite miss). PLANTED remains G9. See [bugs/HUNT-2026-08-24.md](../bugs/HUNT-2026-08-24.md) and [bugs/CLOSE-281.md](../bugs/CLOSE-281.md). There is no remaining “281 still open” item.

## G0 — Ultimate goal

Find (or, as fallback, honestly document a production-class near-miss) a **schedule-dependent** safety violation that a conventional unit-test suite would not write by hand — with:

1. a seed
2. a SHA-256 trace digest that replays byte-for-byte
3. a minimized fault/delivery schedule
4. a root-cause writeup a stranger can follow in one sitting

That package is the [final deliverable](deliverable.md). Everything below is in dependency order toward it.

## Sub-goals

### G1 — Deterministic runtime boundary

Protocol never touches the real world. `step` is `(State, Event) -> Vec<Effect>`. No `Instant`, sockets, files, threads, `HashMap` in `chronos-protocol`. No `HashMap` in `chronos-sim`.

- **G1.1** Event / Effect / IoOp / TimerKind / IoId types exist and match the architecture.
- **G1.2** Clippy + CI grep actually fail a planted `use std::fs` in protocol.
- **G1.3** `chronos-sim` and `chronos-node` both depend on protocol and **not** on each other.

**Done:** crate graph + lint gates. **Phase:** P1 (types), P9 (CI grep in CI).

### G2 — Interpreters, not sync I/O traits

Two interpreters run the same `step`. Disk and net are submit → later `IoComplete` / `MessageReceived`.

- **G2.1** Single-node KV talks only via `step` + effects.
- **G2.2** Sim interpreter (even stub) and node interpreter (real WAL) are swappable without editing KV/Raft.
- **G2.3** `ArmTimer { kind }` — harness/node owns duration and `now`. Protocol has no clock and no RNG.

**Done:** swap test. **Phase:** P1.

### G3 — Deterministic scheduler

One thread. Tickless `u64` ns. Heap key `(time, global seq)`. One seeded PRNG.

- **G3.1** Global seq increments at every enqueue, never per-node.
- **G3.2** Toy protocol (echo/ping) runs in the loop.
- **G3.3** `hash(run(seed)) == hash(run(seed))` twice, CI-gated. SHA-256 of encoded records, no host fields.

**Done:** determinism test. **Phase:** P2.

### G4 — Simulated network

Delay, loss, duplicate, partition. `MsgId` at enqueue.

- **G4.1** Delivery queue keyed by `(delivery_at, seq)`.
- **G4.2** Adjacency matrix; drop if `!connected`.
- **G4.3** Reorder falls out of per-message delay. No extra reorder structure in v1.

**Done:** scripted delay/drop/partition on toy protocol. **Phase:** P2 (queue), P4 (fault knobs).

### G5 — Simulated disk

`bytes` vs `durable_len`. Append-only WAL. Torn suffix **past** `durable_len`. Fsync-lie modeled, default **off**.

- **G5.1** `[len][crc32(len||payload)][payload]`, CRC-32/ISO-HDLC, little-endian.
- **G5.2** `Meta` + `Entry` records. No in-place term/votedFor.
- **G5.3** Appends to one node applied in emission order; completions may reorder.
- **G5.4** Crash: keep durable prefix, tear in-flight suffix, drop in-flight I/O, `Recover` + CRC scan.
- **G5.5** `IoId` incarnation bumps on recover; stale completions ignored.

**Done:** unit tests for CRC, torn tail, fsync-Err, fsync-lie-off. **Phase:** P1 (format), P2 (sim disk), P4 (faults).

### G6 — Raft (election + replication)

Minimal Raft inside `step`. Static cluster. No snapshots. Get via log. No-op on new leadership.

- **G6.1** Election, terms, log matching, nextIndex/matchIndex.
- **G6.2** Persist-before-send (RequestVote and vote-grant). Persist-before-ack (AE success). `PersistCookie` on durable vs volatile.
- **G6.3** Leader does not count self toward commit until own fsync completes.
- **G6.4** Current-term commit only (Figure 8). No-op (or equivalent) at start of term.
- **G6.5** KV apply inside `step`. Idempotency table is SM state. `(client_id, request_id)` on every op.

**Done:** 3 nodes, no faults, one leader, logs match, Get/Put. **Phase:** P3.

### G7 — Executable safety

White-box Raft invariants after every world event. Black-box linearizability on client history. Liveness is a **separate** flag.

- **G7.1** Five Raft safety properties.
- **G7.2** Persist-before-send/ack, commitIndex monotonic, current-term commit, self-matchIndex after fsync.
- **G7.3** Linearizability checker on `{invoke_at, complete_at, result}`. Incomplete ops allowed.
- **G7.4** Planted bug: skip `votedFor` persist → Election Safety fails. Unplanted happy path still passes.
- **G7.5** Liveness only when `T_ns = 10 * max_election_timeout` majority connected; off under permanent partitions.

**Done:** planted bug caught; happy path green. **Phase:** P5.

### G8 — Schedule fuzzing

Search interleavings, not byte buffers. Swarm config from seed.

- **G8.1** Seed → n∈{3,5}, delays, fault probs, buggify, client rate, `max_ns`.
- **G8.2** Mix calm and brutal (not 90% drop forever).
- **G8.3** On fail: `fail-<seed>.trace`, print seed, digest, first check.
- **G8.4** Headless; thousands of seeds is a goal, not a start gate.

**Done:** `chronos-sim --seeds N` runs and logs. **Phase:** P6.

### G9 — Bug artifact

A real (preferred) or documented near-miss (fallback) schedule bug.

- **G9.1** Invariant that broke, fault sequence, why unit tests would miss it.
- **G9.2** Same seed reproduces after a clean checkout of that commit.
- **G9.3** If a real bug: fix it, same seed no longer trips. If near-miss: say so; do not fake a production bug.

**Done:** writeup in `docs/bugs/` (or equivalent). **Phase:** P7.

### G10 — Minimization

Delta-debug the **fault/delivery schedule**.

- **G10.1** Record schedule so replay does not need the PRNG to make the same extra choices after a deletion.
- **G10.2** 10k-event failure → smallest subsequence that still fails.
- **G10.3** Minimized schedule is the human-readable ticket.

**Done:** tool + one minimized example (P7 seed). **Phase:** P8.

### G11 — Product quality

Someone else can run, replay, and trust CI.

- **G11.1** Coverage of which fault *combinations* ran.
- **G11.2** CI: determinism test + N seeds per commit; merge blocked on new invariant fails.
- **G11.3** README stands alone: how to run, how to replay, what is out of scope (D10, Byzantine, membership, snapshots, Jepsen).

**Done:** P9 exit checks. **Phase:** P9.

## Goal → phase map

| Goal | Primary phases |
|------|----------------|
| G1 boundary | P1, P9 |
| G2 interpreters | P1 |
| G3 scheduler + hash | P2 |
| G4 network | P2, P4 |
| G5 disk | P1, P2, P4 |
| G6 Raft + KV | P3 |
| G7 checkers | P5 |
| G8 fuzz | P6 |
| G9 bug writeup | P7 |
| G10 minify | P8 |
| G11 CI/docs | P9 |

## Non-goals (do not grow into goals)

Listed in the architecture: Byzantine, membership changes, snapshots, lease/read-index, multi-DC clock drift, node perf work, hypervisor DST, Jepsen. A later version can add them; v1 must not stall on them.
