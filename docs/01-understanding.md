# Understanding Chronos

Read this before the architecture doc. It is the shared vocabulary so "Raft," "simulation," and "product" do not silently mean different things.

## What we are actually building

A **product** with two user-facing surfaces that share one protocol core:

| Surface | Who uses it | Promise |
|---------|-------------|---------|
| `chronos-sim` | us, CI, an interviewer replaying a seed | Same seed + same commit → byte-identical trace. Invariant violation dumps seed, trace, violating state. |
| `chronos-node` | a human or client talking to a real cluster | The *same* state machine, with OS clock / TCP / files as the interpreter. |

If those two surfaces ever grow different Raft logic, the product is dead. Simulation that tests a cousin of production is how people get a green fuzzer and a production split-brain.

The KV store is the **demo application**. The consensus log is the **engine**. The simulator is the **differentiator**. Interviewers have seen Raft clones. They have not seen a hashed, replayable, minimizable schedule fuzzer with an honest disk model.

## Why distributed systems are hard (DDIA, compressed)

Kleppmann's useful claims for this codebase, not the whole book:

1. **There is no shared memory and no shared clock.** Two nodes can both be right about what *they* saw and still disagree about order. Consensus is how we pick one order.
2. **Partial failure is the normal case.** Timeout means one of: never arrived, arrived and crashed, arrived and the reply was lost. The protocol must be correct for all three. That is why every RPC is idempotent on the server and retried with a request id on the client.
3. **Replication is not durability.** Three in-memory copies die together. Durability is "survives the failures we named," which is why fsync and torn writes are first-class, not ops afterthoughts.
4. **Quorum overlap is the math behind Election Safety.** Majority votes, one vote per term, persisted. Two majorities share a node; that node cannot have voted twice.
5. **Linearizability vs sequential vs eventual.** The KV API we will claim is linearizable: each op takes effect atomically at a point between invoke and response, and that point respects simulated real-time. Eventual consistency is not a Raft KV.
6. **Consensus = total-order broadcast of commands.** Raft is how replicas agree on the next log entry. The KV is a deterministic state machine applied in that order. If apply is not deterministic (HashMap iteration, `now()` in the command), the replicas diverge and the checker is right to scream.
7. **Unreliable clocks.** We will not use wall clocks for safety. Terms and log indices are the logical clock. Virtual time exists only so timeouts and delays have a total order the scheduler can run.
8. **CAP during a partition.** Raft chooses consistency: the minority leader cannot commit. Availability of writes in a minority partition is *supposed* to stop. A "bug" that is "minority cannot write" is not a bug.

## What DST is (and is not)

**Deterministic Simulation Testing** means: every source of nondeterminism that can change protocol behavior is an input to a function of `(seed, code, initial config)`. The OS is not allowed to contribute.

It is **white-box schedule fuzzing**. The search space is interleavings of messages, timeouts, disk completions, crashes, and partitions — not just byte buffers to a parser.

It is **not**:

- Jepsen (black-box, real binaries, sparse, often unreproducible — complementary).
- Chaos engineering in staging (same).
- Unit tests of `RequestVote` given a struct (necessary, not sufficient).
- Antithesis (hypervisor DST for code you cannot instrument — overkill here).

Wilson's warning: the nightmare is a **wrong simulator**. If we model disks as "write is instantly durable," we will never find the persist-before-send bug. If we model fsync-lies as default reality, we will "find" that Raft is broken, which the PAR paper already told us.

## The five Raft safety properties (what "correct" means)

These are the product's contract. They will be executable checks, not README poetry.

1. **Election Safety** — at most one leader per term.
2. **Leader Append-Only** — a leader never overwrites or deletes entries in its own log.
3. **Log Matching** — if two logs have the same index and term, all preceding entries are identical.
4. **Leader Completeness** — if an entry is committed in term T, every leader of term > T has that entry.
5. **State Machine Safety** — no two servers apply different commands at the same index.

Liveness ("a leader is eventually elected if a majority is connected") is a **different** check with **different** fault assumptions. A permanent partition of the majority must not fail a liveness assertion. Mixing the two is how simulators become flaky.

## How a bug actually looks

Not "Raft is wrong." More like:

> At virtual t=4_201_332_000, node 2's fsync for `(term=5, votedFor=2)` is still in the disk queue. The scheduler delivers `RequestVote` from node 2 to nodes 1 and 3 first. Then it crashes node 2. On restart, durable state still has `votedFor=None` for term 5. Node 2 votes for node 1. Two majorities, one term.

A unit test will not write that scenario unless a human already knew it. A schedule fuzzer will, if and only if **persist and send are separate effects** the scheduler can split.

That last sentence is the entire architecture for *votes*. A leader may `Append` a log entry and `Send` `AppendEntries` in the same batch — that is legal Raft. The node must not count itself committed until the fsync completes, and a follower must not ack until *its* persist completes. See persist-before-send / persist-before-ack in [02-architecture.md](02-architecture.md).

## Product constraints (so we do not build a lab toy)

- **Replay is a feature.** Seed + git commit + trace file. If we cannot attach a debugger to a failure, we failed the product, not just a test.
- **Minimization is a feature.** A 10k-event failure no one will read. A 12-event failure is a ticket.
- **Coverage is a feature.** Which fault *combinations* ran, not a green checkmark.
- **Honesty about scope is a feature.** We do not claim Byzantine tolerance, fsync-lies, or multi-DC asynchronous replication until the protocol actually handles them.
- **Windows + Linux.** This repo is being started on Windows. No `libc` preload, no Linux-only runtime shim. The protocol must be deterministic because *we wrote it that way*, not because a hypervisor trapped `getrandom`.

## Skill levels this product is meant to prove

| Level | Proof artifact |
|-------|----------------|
| 1 Implement | Compiles; same seed twice → identical trace hash; happy-path election + replication. |
| 2 Understand | You can predict a partition scenario *before* running it, then watch the sim confirm. |
| 3 Correctness | Executable invariants; a real (or planted-then-real) bug with minimized trace and root cause; a written list of what the sim does not cover. |

Phase 7 of the project guide (find a real bug) is the portfolio punchline. Everything before that exists to make that punchline *possible*, not to look like a complete distributed database.
