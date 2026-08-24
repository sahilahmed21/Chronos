# Research Notes

*Generated: 2026-08-18. Firecrawl/Exa MCP were not available; sources were gathered via web search and full-page reads. Confidence: high on architectural lessons, medium on unpublished internals (TigerBeetle VOPR source layout, Antithesis hypervisor details).*

This is not a literature review for its own sake. Every source below changes a concrete decision in [02-architecture.md](02-architecture.md).

## Sub-questions investigated

1. How do production systems that *ship* DST actually structure the I/O boundary?
2. What simulator shape (own event loop vs Tokio-shaped runtime) survives contact with a real protocol?
3. What storage faults does vanilla Raft survive, and which ones require a different protocol?
4. Which Raft "details" are actually load-bearing safety, not implementation trivia?
5. What hidden nondeterminism will make `same seed ≠ same trace` on this machine (Windows) and in CI (Linux)?

## 1. FoundationDB: simulator first, not database first

FoundationDB spent the first ~18 months building in simulation, before the database sent a real packet ([Engineering docs](https://apple.github.io/foundationdb/engineering.html), [Testing docs](https://apple.github.io/foundationdb/testing.html)). Will Wilson's talk ([notes](http://alex-ii.github.io/notes/2018/04/29/distributed_systems_with_deterministic_simulation.html)) names three ingredients, not twenty:

1. **Single-threaded pseudo-concurrency.** Threads inside the protocol *are* nondeterminism. The simulator is one OS thread with a time-ordered queue.
2. **Simulated implementations of every external interface.** `INetwork` / `IAsyncFile` / clock. Production and sim satisfy the same interfaces.
3. **The process under test must itself be deterministic.** Ambient `now()`, disk-space checks, and unseeded RNG are bugs in the program, not in the test.

They also named things we should copy as *product* features, not as Phase 9 polish:

- **Determinism self-check:** run the same seed twice; compare RNG consumption / trace hash. Not foolproof, but it catches the usual leaks.
- **Buggification:** internally legal but unusual behavior (extra delay, extra error, weird tuning knobs) so correctness cannot accidentally depend on a happy-path timing.
- **Swarm testing:** each run randomizes cluster size, workload, fault rates, and which buggify points are on ([FDB paper](https://www.foundationdb.org/files/fdb-paper.pdf)).
- **Fault-rate tuning:** too many faults collapse the state space into "cluster is always dead." Fuzzing must spend time in the interesting middle.
- **Swizzle-clogging:** clog a random subset of connections one-by-one, then unclog in a different random order. Better at finding deep bugs than "partition everyone at once."
- **Nightmare case:** the simulator is wrong — not brutal enough, or it misunderstands the OS contract. Mitigation is a real-hardware "sinkhole," not more mock fidelity. For Chronos, that is a later *complement* (Jepsen-class), not v1.

**Implication:** write the simulator substrate before Raft. A toy protocol that only echoes messages is enough to prove `same seed → same trace hash`.

## 2. TigerBeetle VOPR: production code in the sim, tickless time

TigerBeetle's VOPR ([internals doc](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/internals/vopr.md), [browser writeup](https://tigerbeetle.com/blog/2023-07-11-we-put-a-distributed-database-in-the-browser/)) runs **the production replica code** in one process with clock/network/disk stubbed. Replay key is `(seed, git commit)`.

The 2025 "tickless VOPR" work ([PR 2956](https://github.com/tigerbeetle/tigerbeetle/pull/2956), [PR 2846](https://github.com/tigerbeetle/tigerbeetle/pull/2846)) is the rework we are refusing in advance:

- Discrete integer **ticks** force many I/O completions to share a timestamp. You then have to shuffle "ready this tick" by hand.
- Modeling latency in **nanoseconds** makes random timestamps shuffle events naturally, and maps onto real hardware latencies later (perf sim, not just safety sim).
- The pending-event queue is the source of truth. Do not keep a parallel "ready list" that can desync.

They also split **safety mode** (probabilistic faults) from **liveness mode** (quorum available, partitions heal). Checking liveness under a permanent partition is a false failure.

**Implication:** Chronos time is `u64` nanoseconds from 0. No tick counter. Safety checks and liveness checks are different knobs.

## 3. Dropbox Nucleus / Trinity: design away invalid states, intercept completions

Nucleus testing ([Dropbox](https://dropbox.tech/infrastructure/-testing-our-new-sync-engine)) is DST for a sync engine, not Raft, but the mechanical lessons transfer:

- Trinity runs on the **same thread** as the engine, intercepts async requests, and **reorders completions**.
- The in-memory filesystem can snapshot/restore to simulate crashes.
- Determinism is verified by **re-running the seed and asserting the same final state**.
- "Design away invalid states" in the type system beats checking for them after the fact.

**Implication:** disk/network *completions* are events the harness can delay, reorder, or drop. Two `Append`s to the same WAL are applied in emission order (reordering them would scramble CRC boundaries). Crash = keep the durable prefix, optionally tear the in-flight suffix past `durable_len`, discard volatile. Protocol types should make "two leaders in the same term" unrepresentable as a *cluster fact* (it can still exist as a partitioned *belief*).

## 4. MadSim / RisingWave / Turmoil: do not borrow their *shape*

[MadSim](https://github.com/madsim-rs/madsim) (RisingWave) and [Turmoil](https://github.com/tokio-rs/turmoil) prove that Rust *can* DST an existing Tokio app by replacing the runtime. RisingWave's writeup is honest about the cost: patch `getrandom` / `quanta` / `tokio`, compile with `--cfg madsim`, and still fight flakiness when "ready" is a sleep instead of a state poll ([Part 2](https://risingwave.com/blog/applying-deterministic-simulation-the-risingwave-story-part-2-of-2/)).

A 2026 practitioner writeup on Turmoil is blunter: `HashMap` iteration, `Instant::now`, and transitive `getrandom` leak entropy even when the runtime is mocked ([retry-race post](https://www.tiarebalbi.com/en/blog/catching-retry-race-deterministic-simulation-rust-turmoil)).

MadSim also defaults to **forbidding OS threads** because they break determinism.

**Implication for Chronos:** we are greenfield. Using MadSim/Turmoil would optimize for "looks like Tokio" and fight Windows (`libc` shims), HashMap, and a protocol that should not be async in the first place. Own the scheduler. That is also the thing an interviewer can actually audit.

## 5. Antithesis: complementary, not a substitute

A [deterministic hypervisor](https://antithesis.com/blog/deterministic_hypervisor/) makes an unmodified container reproducible by intercepting syscalls, clocks, and scheduling. That is the right tool when you **cannot** control the I/O boundary.

Chronos *can*. In-process DST is cheaper, faster, and teaches the architecture. Hypervisor DST / Jepsen remain the way to catch "our sim misunderstood the kernel." We do not build a hypervisor.

## 6. Raft load-bearing details (not trivia)

These are the bugs DST is for. Encode them as checker rules and as effect-ordering laws, not as comments.

- **Persist `currentTerm` and `votedFor` before sending or answering votes.** sofa-jraft shipped the classic bug: send `RequestVote`, *then* persist. Crash in between → double vote in one term ([PR 1245](https://github.com/sofastack/sofa-jraft/pull/1245)). Same class: mutate term in memory and forget to persist ([raft-java #57](https://github.com/wenweihu86/raft-java/issues/57)).
- **Figure 8 / old-term commit.** A leader must not commit previous-term entries by counting replicas. Only a *current-term* entry committed by majority count commits prior entries indirectly (Log Matching). Missing this looks like "replication works" until a later election overwrites a majority-replicated entry. New leaders should append a no-op (or wait for a client write) before committing anything.
- **Linearizable reads are not `Get` on the leader's memory.** At the start of a term the leader may not know the commit index; a deposed leader may still think it is leader. Ongaro's rule: commit a no-op, then either read via the log or read-index (heartbeat majority + wait for apply) ([raft-dev](https://groups.google.com/g/raft-dev/c/4QlyV0aptEQ)). v1 of Chronos puts **Get in the log** so the checker is honest.
- **Client retries need idempotency keys.** At-least-once RPC + a non-idempotent `Put` is a linearizability violation that is not a Raft bug. DDIA's rule: exactly-once = at-least-once + idempotence. Every command carries `(client_id, request_id)`.

## 7. Storage faults: Raft vs Protocol-Aware Recovery

The user's spec wants torn writes *and* "fsync that lies." Those are different products.

[Protocol-Aware Recovery (FAST '18)](https://www.usenix.org/system/files/conference/fast18/fast18-alagappan.pdf) showed that LogCabin (Raft) and ZooKeeper (ZAB) are **unsafe or unavailable** under realistic storage faults (corruption, misdirected writes, torn sectors) if recovery is "generic checksum + panic." CTRL adds: detect crash-torn vs bitrot, recover committed entries from replicas, discard uncommitted ones for availability.

**Implication:**

| Fault | Vanilla Raft + checksummed WAL | Chronos v1 fuzzer |
|-------|--------------------------------|-------------------|
| Crash before fsync | Survive (durable prefix only) | On by default |
| Torn tail of an append | Survive if records are length+CRC and we truncate to last good record | On by default — **only past `durable_len`** (in-flight suffix). Not an fsync-lie. |
| fsync returns success but bytes never hit media | **Not survived.** Paper Raft assumes stable storage is stable. | Modeled, **off** until recovery exists |
| Silent bitrot of a committed entry | Needs replica repair (PAR/CTRL), not just Raft | Later |

Shipping "fsync lies" in the default fuzzer would "find" a bug that is actually a **protocol limitation**. That wastes the Phase 7 artifact. Model the fault. Do not claim Raft handles it.

## 8. Linearizability checking is a second, independent oracle

[Porcupine](https://github.com/anishathalye/porcupine) (built for MIT's Raft KV lab) checks a **client history** against a sequential spec. That is not the same as "Election Safety after every event."

- White-box Raft invariants catch protocol bugs early (double leader in a term, log mismatch).
- Black-box linearizability catches "Raft looks fine, KV API is still wrong" (lost update, stale read, duplicate apply).

Chronos needs both. The history is recorded from simulated clients with virtual timestamps between invoke and complete — the linearization point must lie in that interval.

## 9. Rust-specific entropy (this repo, this OS)

Default `std::collections::HashMap` is randomly seeded from the OS ([std docs](https://doc.rust-lang.org/stable/std/collections/struct.HashMap.html)). Iteration order changes between runs. On Windows *and* Linux CI this will fail a determinism test even if the scheduler is perfect.

Banned in `chronos-protocol`: `Instant::now`, `SystemTime::now`, `thread::spawn`, `getrandom`, `f64` for time, `HashMap`/`HashSet`, `std::fs`, sockets.

Banned in `chronos-sim` as well: `HashMap`/`HashSet`. The simulator's net/disk/checker/trace iteration is what the trace hash sees. A deterministic hasher is not the gate; `BTreeMap`/`BTreeSet` is.

Use `BTreeMap` / `BTreeSet`. One PRNG. Integer nanoseconds. Clippy `disallowed_types` + CI grep; Cargo will not catch `use std::fs`.

## What we are not taking

- **Flow / actors as a language.** We get the same shape with an explicit `step` function and a queue. Less magic, easier to audit.
- **MadSim drop-in Tokio.** Wrong boundary for a greenfield protocol.
- **Byzantine faults.** Raft does not claim them. Testing for them proves nothing about this protocol.
- **Real clock drift as a continuous analog process.** We can inject discrete clock jumps later. Continuous drift is a hypervisor problem.

## Sources

1. [FoundationDB Simulation and Testing](https://apple.github.io/foundationdb/testing.html) — cluster-in-one-thread, time dilation, fault zoo.
2. [FoundationDB Engineering](https://apple.github.io/foundationdb/engineering.html) — Flow + 18 months in sim before real packets.
3. [Will Wilson talk notes](http://alex-ii.github.io/notes/2018/04/29/distributed_systems_with_deterministic_simulation.html) — three ingredients; buggify; sinkhole nightmare case.
4. [FoundationDB paper](https://www.foundationdb.org/files/fdb-paper.pdf) — swarm testing, buggification, fault-rate tuning, conditional coverage.
5. [TigerBeetle VOPR](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/internals/vopr.md) — production code in sim; seed + commit replay.
6. [TigerBeetle in the browser](https://tigerbeetle.com/blog/2023-07-11-we-put-a-distributed-database-in-the-browser/) — VOPR origin (FDB + Dropbox Nucleus).
7. [Tickless VOPR PR 2956](https://github.com/tigerbeetle/tigerbeetle/pull/2956) — nanosecond time, not ticks.
8. [Dropbox Nucleus testing](https://dropbox.tech/infrastructure/-testing-our-new-sync-engine) — same-thread completion reordering; crash via filesystem snapshot.
9. [MadSim](https://github.com/madsim-rs/madsim) / [RisingWave Part 1](https://risingwave.com/blog/deterministic-simulation-a-new-era-of-distributed-system-testing/) / [Part 2](https://risingwave.com/blog/applying-deterministic-simulation-the-risingwave-story-part-2-of-2/) — Tokio-shaped DST; why we will not copy the shape.
10. [Turmoil + HashMap leak](https://www.tiarebalbi.com/en/blog/catching-retry-race-deterministic-simulation-rust-turmoil) — runtime mock is not enough.
11. [Antithesis deterministic hypervisor](https://antithesis.com/blog/deterministic_hypervisor/) — black-box DST; complementary.
12. [sofa-jraft persist-before-send](https://github.com/sofastack/sofa-jraft/pull/1245) — the bug class our effect ordering must make checkable.
13. [Protocol-Aware Recovery](https://www.usenix.org/system/files/conference/fast18/fast18-alagappan.pdf) — fsync-lies / bitrot vs paper Raft.
14. [Porcupine](https://github.com/anishathalye/porcupine) — client-history linearizability checker.
15. [raft-dev linearizable reads](https://groups.google.com/g/raft-dev/c/4QlyV0aptEQ) — no-op + read-index.
16. Kleppmann, *Designing Data-Intensive Applications* — replication, quorums, linearizability, "exactly-once," unreliable clocks, consensus as total-order broadcast.

## Methodology

Searched 12+ queries across web and docs. Deep-read FoundationDB testing/engineering pages, Wilson notes, FDB paper excerpts, PAR paper, Nucleus testing page, MadSim/RisingWave, Antithesis DST docs. Sub-questions listed above. Gaps: live TigerBeetle `vopr.md` fetch returned empty (GitHub blob); relied on blog + PR discussion. No Exa/Firecrawl (not configured).
