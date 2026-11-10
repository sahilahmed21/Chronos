# Understanding Chronos

Read this before the architecture doc. It establishes the shared vocabulary so terms like "Raft," "simulation," and "product" have precise meaning throughout the codebase and documentation.

## What Chronos is

A **system with two user-facing surfaces that share one protocol core**:

| Surface | Who uses it | Promise |
|---------|-------------|---------|
| `chronos-sim` | developers, CI, anyone replaying a seed | Same seed + same commit → byte-identical trace. An invariant violation dumps the seed, trace, and violating state. |
| `chronos-node` | a client talking to a real node | The *same* state machine, with OS clock / TCP / files as the interpreter. |

If those two surfaces ever diverge in their Raft logic, the product is broken. Simulation that tests a cousin of production is how teams get a green fuzzer and a production split-brain.

The KV store is the **demo application**. The consensus log is the **engine**. The simulator is the **differentiator**. Raft implementations are common; a hashed, replayable, minimizable schedule fuzzer with an honest disk model is not.

## Why distributed systems are hard (DDIA, compressed)

Kleppmann's useful claims for this codebase, not the whole book:

1. **There is no shared memory and no shared clock.** Two nodes can both be right about what *they* saw and still disagree about order. Consensus is how nodes pick one order.
2. **Partial failure is the normal case.** Timeout means one of: never arrived, arrived and crashed, arrived and the reply was lost. The protocol must be correct for all three. That is why every RPC is idempotent on the server and retried with a request id on the client.
3. **Replication is not durability.** Three in-memory copies die together. Durability is "survives the named failures," which is why fsync and torn writes are first-class concerns, not ops afterthoughts.
4. **Quorum overlap is the math behind Election Safety.** Majority votes, one vote per term, persisted. Two majorities share a node; that node cannot have voted twice.
5. **Linearizability vs sequential vs eventual.** The KV API Chronos exposes is linearizable: each op takes effect atomically at a point between invoke and response, and that point respects simulated real-time. Eventual consistency is not a Raft KV.
6. **Consensus = total-order broadcast of commands.** Raft is how replicas agree on the next log entry. The KV is a deterministic state machine applied in that order. If apply is not deterministic (HashMap iteration, `now()` in the command), the replicas diverge and the checker is right to flag it.
7. **Unreliable clocks.** Wall clocks are not used for safety. Terms and log indices are the logical clock. Virtual time exists only so timeouts and delays have a total order the scheduler can run.
8. **CAP during a partition.** Raft chooses consistency: the minority leader cannot commit. Unavailability of writes in the minority partition is *correct* behavior, not a bug.

## What DST is (and is not)

**Deterministic Simulation Testing** means: every source of nondeterminism that can change protocol behavior is an input to a function of `(seed, code, initial config)`. The OS is not allowed to contribute.

It is **white-box schedule fuzzing**. The search space is interleavings of messages, timeouts, disk completions, crashes, and partitions — not just byte buffers to a parser.

It is **not**:

- Jepsen (black-box, real binaries, sparse, often unreproducible — complementary).
- Chaos engineering in staging (same).
- Unit tests of `RequestVote` given a struct (necessary, not sufficient).
- Antithesis (hypervisor DST for code you cannot instrument — a complement, not a substitute).

Wilson's warning: the nightmare is a **wrong simulator**. If disks are modeled as "write is instantly durable," the persist-before-send bug will never be found. If fsync-lies are on by default, the simulator will "find" that Raft is broken — which the PAR paper already established as a protocol-level limitation, not a bug.

## The five Raft safety properties (what "correct" means)

These are the system's correctness contract. They are implemented as executable checks, not documentation.

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

## Design constraints

- **Replay is a feature.** Seed + git commit + trace file. A failure that cannot be attached to a debugger is a failure of the product, not just a test.
- **Minimization is a feature.** A 10k-event failure no one will read. A 12-event failure is a ticket.
- **Coverage is a feature.** Which fault *combinations* fired, not just a green checkmark.
- **Honesty about scope is a feature.** Byzantine tolerance, fsync-lies, and multi-DC replication are not claimed until the protocol actually handles them.
- **Windows + Linux.** No `libc` preload, no Linux-only runtime shim. The protocol is deterministic because it is written that way, not because a hypervisor trapped `getrandom`.
