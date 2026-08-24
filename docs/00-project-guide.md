# Deterministic Simulation Testing (DST) on a Raft KV Store — Full Project Guide

This is the original project specification, saved verbatim as the source of intent. Architecture that we will actually build (and will not rewrite) lives in [02-architecture.md](02-architecture.md). If this guide and that file disagree (synchronous `Disk::write`, blanket persist-before-send, integer ticks, HashMap-only-in-protocol, and so on), **02-architecture.md wins**.

---

## 1. Problem Statement

**The bug class you're targeting:** distributed systems fail in ways that are combinatorial in nature — a bug only appears when a specific *interleaving* of events happens: message A arrives 3ms before message B, the disk fsync stalls for exactly 40ms during a leader election, a process crashes mid-write. These bugs are:

- **Rare** — the triggering interleaving might occur once in 10 million runs in production.
- **Non-reproducible** — you can't attach a debugger to a bug that only happens under a specific race condition across three machines.
- **Expensive to find via traditional testing** — unit tests check "does the code do X given input Y," not "does the *protocol* stay correct under all possible schedules of network/disk/time events."

Traditional approaches (unit tests, integration tests, even chaos engineering / Jepsen) either don't explore the schedule space systematically, or when they do find a bug, can't give you a deterministic, replayable trace back to the exact failure.

**The problem you are solving:** build a system where the *entire distributed protocol* (Raft, in this case) runs inside a simulated environment where you — not the OS, not the network card, not the disk controller — control every source of nondeterminism. This lets you:

1. Fuzz over thousands/millions of possible event schedules cheaply (single machine, single thread, no real network).
2. When something breaks a correctness invariant, replay the *exact* seed and get byte-for-byte the same failure, every time.
3. Turn "this bug happened once in staging and we can't reproduce it" into "here is a 12-line minimized trace that reproduces the linearizability violation in 4ms."

This is fundamentally an exercise in **architecting away nondeterminism**, not "writing more tests."

---

## 2. Proposed Architecture

### 2.1 High-level shape

```
┌─────────────────────────────────────────────────────────┐
│                     Simulation Harness                    │
│  (seeded PRNG drives every decision below)                 │
│                                                             │
│  ┌───────────────┐   ┌───────────────┐   ┌─────────────┐ │
│  │ Deterministic  │   │  Simulated     │   │  Simulated  │ │
│  │ Event-Loop     │◄─►│  Network       │   │  Disk       │ │
│  │ Scheduler      │   │  (delay/loss/  │   │  (torn      │ │
│  │                │   │  partition/    │   │  writes,    │ │
│  │                │   │  reorder)      │   │  fsync      │ │
│  │                │   │                │   │  failure)   │ │
│  └───────┬────────┘   └───────┬────────┘   └──────┬──────┘ │
│          │                    │                    │        │
│          ▼                    ▼                    ▼        │
│   ┌────────────────────────────────────────────────────┐  │
│   │        N Raft Node Instances (logically distinct,   │  │
│   │        but running in one OS thread)                │  │
│   └────────────────────────────────────────────────────┘  │
│                          │                                  │
│                          ▼                                  │
│              ┌────────────────────────┐                     │
│              │  Invariant / Property   │                     │
│              │  Checker (runs after    │                     │
│              │  every event)           │                     │
│              └────────────────────────┘                     │
└─────────────────────────────────────────────────────────┘
```

### 2.2 The core architectural rule

**Every piece of code that touches "the real world" — `SystemTime::now()`, a TCP socket, a file `write()`/`fsync()`, thread spawning — must go through a trait/interface, never called directly.** This is the single most important design decision. Concretely:

- `Clock` trait: `fn now(&self) -> Timestamp`. Production impl wraps `SystemTime`; sim impl is a virtual clock advanced only by the scheduler.
- `Network` trait: `fn send(&self, to: NodeId, msg: Msg)`. Production impl is real sockets; sim impl enqueues into a priority queue keyed by a simulated delivery time.
- `Disk` trait: `fn write(&self, offset, bytes) -> Result<()>`, `fn fsync(&self) -> Result<()>`. Production impl is real file I/O; sim impl is an in-memory byte array that can be told to "tear" a write (apply only the first K bytes) or fail an fsync.
- **No `std::thread::spawn`, no `tokio::spawn` with real reactor** inside the core protocol logic. The Raft state machine should be a pure(ish) function: `(State, Event) -> (State, Vec<Effect>)`. Effects (send message, write to disk, set a timer) are *interpreted* by the harness — real harness does it for real, sim harness does it deterministically.

### 2.3 Simulated Network design

- Priority queue of `(delivery_time, message)`.
- On `send()`, the seeded PRNG decides: drop it? duplicate it? delay it (and by how much)? deliver in order or reorder relative to other in-flight messages?
- Partitions are modeled as a symmetric or asymmetric adjacency matrix between nodes, toggled by the fault-injection schedule (e.g., "at simulated tick 4200, partition {node1} from {node2, node3}").

### 2.4 Simulated Disk design

- Each node's on-disk state (Raft log, `currentTerm`, `votedFor`) is a byte buffer plus a "durable up to offset X" pointer.
- A `write()` call updates the in-memory buffer immediately but only advances the durable pointer on `fsync()`.
- Fault injection can: (a) crash the node before `fsync()` — on recovery, only bytes before the last durable pointer are visible (models a torn write); (b) fail `fsync()` outright while pretending to the caller it succeeded (models firmware lies) — this is the nastiest and most realistic class of fault.

### 2.5 Deterministic Event-Loop Scheduler

- Single-threaded. Maintains one global priority queue of "pending events" (timer fires, message deliveries, disk completions).
- Pops the earliest event, advances virtual time to that event's timestamp, executes it, lets it produce new events, repeats.
- **No OS thread scheduling is ever involved in protocol logic** — this is what makes two runs with the same seed byte-identical, regardless of what machine you run them on.
- All "randomness" (which node's timer fires first when two are tied, which message gets dropped, etc.) comes from a single seeded PRNG, so the entire run is a pure function of the seed.

### 2.6 Raft Layer

- Either implement a minimal Raft (leader election + log replication + no snapshotting to start) or take an existing minimal implementation and adapt its I/O boundary to your traits.
- Persisted state per node: `currentTerm`, `votedFor`, `log[]`.
- Volatile state: `commitIndex`, `lastApplied`, and on leaders: `nextIndex[]`, `matchIndex[]`.

### 2.7 Invariant / Property Checker

Runs after every simulated event, checking things like:

- **Election Safety**: at most one leader per term.
- **Leader Append-Only**: a leader never overwrites or deletes entries in its own log.
- **Log Matching**: if two logs contain an entry with the same index and term, all preceding entries are identical.
- **Leader Completeness**: if an entry is committed in term T, it's present in the log of every leader of term > T.
- **State Machine Safety** (the linearizability-adjacent one): if a server has applied a log entry at a given index to its state machine, no other server will ever apply a different entry for that index.

---

## 3. How It Will Work — End-to-End Run

1. Harness reads a **seed** (e.g., `42`).
2. Seed initializes: PRNG, initial cluster config (N nodes), fault-injection schedule (can itself be randomized from the seed — e.g., "partition events occur with probability p per tick").
3. Scheduler runs the event loop: nodes send RequestVote/AppendEntries RPCs (as `Effect`s), the simulated network decides delivery/delay/drop, simulated disk decides what's actually durable after each write, timers fire (election timeouts, heartbeats).
4. After every state transition, the property checker asserts all invariants. The moment one is violated, the harness **halts and dumps**: seed, full event trace up to that point, and the violating state.
5. You **replay the identical seed** — because everything is deterministic, you get the exact same trace, letting you attach a real debugger/print statements without the bug "disappearing" (the classic Heisenbug problem in real distributed testing).
6. **Trace minimization**: given a failing 10,000-event trace, binary-search / delta-debug down to the smallest subsequence of events that still reproduces the violation (analogous to `git bisect`, applied to an event log instead of commits).
7. **Fuzzing loop**: run thousands of seeds automatically (CI), each with randomized fault-injection parameters, looking for any seed that trips an invariant.

---

## 4. Points You Are Proving (to yourself and to an interviewer)

1. **You can architect for testability, not just write code that "works."** Pluggable I/O boundaries is a discipline most engineers never practice.
2. **You understand nondeterminism as a first-class engineering concern**, not an inconvenience to be shrugged off as "flaky tests."
3. **You understand a real consensus protocol deeply enough to specify its invariants formally** — not just "I implemented Raft," but "I can tell you exactly what safety properties Raft guarantees and how to check them programmatically."
4. **You can build a fuzzer that searches a *schedule* space, not just an input space** — this is a meaningfully harder and rarer skill than typical fuzzing (e.g., AFL-style byte-fuzzing).
5. **You can go from "found a bug" to "produced a minimal, reproducible, root-caused artifact"** — this is the actual job of a senior distributed systems / infra engineer, not just finding bugs but making them tractable for someone else to fix.
6. **You understand why this technique is complementary to (not a replacement for) Jepsen-style black-box testing** — DST is white-box and exhaustive-ish over schedules but requires you to control the whole system; Jepsen is black-box and tests real deployed binaries but samples the schedule space much more sparsely and less reproducibly.

---

## 5. Skill Levels — What Each Level Actually Proves

**Level 1 — Can you implement?**

- Can you write working Rust/Go/whatever that compiles, passes basic tests, and correctly implements Raft's RPCs and state transitions per the paper?
- Can you build the simulator scaffolding (event loop, sim network, sim disk) so it actually runs deterministically (same seed → same trace, verified by literally diffing two runs)?

**Level 2 — Do you understand distributed systems?**

- Can you explain *why* each Raft mechanism exists (terms, votedFor persistence, log matching property) rather than just having transcribed the paper into code?
- Can you predict, before running the simulator, what *should* happen under a given fault scenario (e.g., "if I partition the leader from majority, within one election timeout a new leader should emerge in the majority partition, and the old leader should step down upon rejoining")?

**Level 3 — Can you reason about correctness?**

- Can you take an informal English safety property ("only one leader per term") and turn it into a checkable, executable invariant?
- Can you diagnose *why* an invariant was violated by reading a minimized trace — i.e., actually do the root-cause analysis, not just observe "test failed"?
- Can you argue about what your simulator does *not* cover (e.g., Byzantine faults, real clock drift, multi-datacenter asymmetric partitions) and why that's an acceptable/deliberate scope boundary?

---

## 6. Ultimate Project Goal, Decomposed

**Ultimate goal:** *Find and document a real, non-trivial correctness bug that your DST harness catches and that a conventional unit-test suite would not — with a byte-reproducible seed, a minimized trace, and a clear root-cause explanation.*

Decomposed sub-goals, in dependency order:

1. **Deterministic runtime** — build the substrate: eliminate ambient nondeterminism (clock, network, disk, threading all behind traits).
2. **Build the deterministic scheduler** — single event loop, seeded PRNG, virtual time.
3. **Simulated network** — delay/loss/reorder/duplicate/partition, all seed-driven.
4. **Simulated disk** — torn writes, fsync failure injection.
5. **Implement Raft** — leader election + log replication, wired through the I/O traits (no bypassing them).
6. **Define invariants** — encode Raft's five safety properties as executable checks.
7. **Fuzz the schedule, not just the input** — randomize fault-injection parameters per seed, run at scale.
8. **Find an actual bug** — something a plain unit test wouldn't catch (e.g., a narrow race between a fsync failure and a concurrent election).
9. **Find an actual fix** — patch it, and re-run the same seed to confirm the invariant no longer trips.
10. **Build trace minimization** — delta-debug the failing trace down to a minimal reproducer.
11. **Build a state-machine checker** — an oracle that, given the sequence of applied log entries across all nodes, verifies State Machine Safety independent of the live invariant checks (a second, independent verification layer).

---

## 7. Phases (Roadmap)

| Phase | Deliverable | "Done" criteria |
|---|---|---|
| **1. Foundation** | I/O traits for Clock/Network/Disk; a trivial single-node WAL-based KV store using them | Store works with a *real* impl of the traits; swapping in a stub sim impl doesn't require touching business logic |
| **2. Simulator** | Deterministic event-loop scheduler; sim network; sim disk; seeded PRNG driving all of it | Running the same seed twice produces byte-identical event traces (write a test that asserts this) |
| **3. Raft** | Minimal Raft (election + replication) running *inside* the simulator across N nodes | A basic scenario (no faults) converges: one leader elected, log replicated, all nodes agree |
| **4. Fault injection** | Partition/delay/loss on network; torn writes/fsync failure on disk, all seed-parameterized | You can specify a fault schedule (e.g., "partition leader at tick 500 for 300 ticks") and watch Raft react correctly |
| **5. Property checking** | Executable checks for all 5 Raft safety properties, running after every event | Checker catches an intentionally-introduced bug (e.g., temporarily remove `votedFor` persistence and confirm the checker flags a double-vote) |
| **6. Fuzzing** | Harness that runs thousands of randomized seeds/fault schedules, headless, in CI | Can run e.g. 10,000 seeds in a reasonable time on one machine; failures are logged with their seed |
| **7. Bug discovery** | An actual invariant violation found via fuzzing, not planted | Documented: what invariant broke, what fault sequence caused it, why |
| **8. Minimization** | Delta-debugging / trace-shrinking tool | 10,000-event failing trace reduced to the smallest event subsequence that still reproduces it |
| **9. Production quality** | Coverage report (which fault *combinations* have been exercised), CI integration (N seeds per commit), documentation of scope/limitations | A README that could stand alone as a portfolio artifact; CI actually blocks merges on new invariant violations |

---

## 8. Interview Prep — Full Question Bank with Detailed Answers

### 8.1 Distributed Systems Fundamentals

**What makes a distributed system difficult?**
Multiple independent failure domains (process, machine, network, disk) that can fail *partially and independently*, combined with no shared global clock and no shared memory — so any two nodes' view of "what happened and in what order" can legitimately disagree unless you engineer agreement (consensus) on top.

**What is a network partition?**
A subset of nodes becomes unable to communicate with another subset (messages between the groups are delayed indefinitely or dropped), while communication *within* each subset may still work fine. Neither side can distinguish "the other side is partitioned" from "the other side crashed" — this ambiguity is the root of most distributed-systems difficulty.

**Why is partial failure difficult?**
Because a node issuing a request can't tell the difference between "the remote node never received my request," "it received it but crashed before responding," and "it received it, processed it, and the response was lost in transit." These three cases require different recovery actions, but look identical to the caller (a timeout).

**What is linearizability?**
The strongest common consistency model: operations appear to take effect atomically at some point between their invocation and response, and that point respects real-time ordering across all clients — i.e., the system behaves as if there's a single, correctly-ordered copy of the data, even though it's replicated.

**What is sequential consistency?**
Weaker than linearizability: all operations appear to execute in *some* total order that's consistent with each individual client's program order, but that total order doesn't have to respect real-time (cross-client) ordering. Two operations that are actually concurrent in real time can be reordered in the agreed-upon sequence, as long as no single client's own operations are reordered relative to each other.

**What is eventual consistency?**
The weakest common guarantee: if no new writes occur, all replicas will *eventually* converge to the same value — but there's no bound on when, and reads in the meantime can return stale or even out-of-order values.

**What is quorum?**
A quorum is a subset of nodes (typically a majority, `⌊N/2⌋+1`) whose agreement is required for an operation to be considered successful. Using majority quorums for both reads and writes guarantees any read quorum and any write quorum overlap in at least one node, which is what lets you always see the latest write.

**What is CAP actually saying?**
During a network partition, a system must choose between Consistency (every read gets the most recent write, or an error) and Availability (every request gets a non-error response, possibly stale). You can't have both *during the partition* — outside of a partition, you can have both. It's a statement about a specific failure mode, not a general "pick 2 of 3" as it's often mis-stated.

**Why doesn't replication automatically provide durability?**
Because if all replicas can be affected by the same correlated failure (power outage, correlated disk firmware bug, or — the classic — a write that a client believes succeeded but wasn't actually fsynced anywhere before a crash touches all replicas that had it only in memory), the data isn't actually durable. Durability requires the data to survive the specific failure modes you care about, which replication alone doesn't guarantee unless combined with proper fsync discipline and geographic/failure-domain diversity.

**What does consensus solve?**
Getting a set of nodes, some of which may fail, to agree on a single value (or a single ordered sequence of values/commands) despite failures and message delays — such that once a value is agreed upon, it's agreed upon durably and all correct nodes eventually learn it.

### 8.2 Raft-Specific Questions

**Why does Raft need terms?**
Terms act as a logical clock that lets nodes detect stale information. Every term has at most one leader, and any message carrying an older term is immediately recognizable as stale — this is what lets followers and candidates safely reject outdated leaders or votes without needing real synchronized clocks.

**Why can there only be one leader per term?**
Because leader election requires a majority vote, and each node casts at most one vote per term. Since two majorities out of the same node set must overlap by at least one node, and that node can only vote once in a given term, it's mathematically impossible for two candidates to both win a majority in the same term.

**How does leader election work?**
A follower that doesn't hear from a leader within its (randomized) election timeout becomes a candidate: increments its term, votes for itself, and sends RequestVote RPCs to all peers. Peers grant a vote if they haven't already voted in this term and the candidate's log is at least as up-to-date as their own. If the candidate gets votes from a majority, it becomes leader and starts sending heartbeats (empty AppendEntries) to assert authority and reset others' timeouts.

**What happens when two candidates request votes (split vote)?**
If neither gets a majority (votes split between them), both election timeouts eventually expire, terms increment again, and the process restarts — randomized election timeouts make it statistically unlikely for repeated splits, since one candidate usually times out and retries first, getting a head start.

**What happens when a stale leader comes back (e.g., after a partition heals)?**
It will discover, via any RPC it sends or receives, that its term is lower than the current term — it immediately steps down to follower state and updates its term. It cannot commit any new entries while partitioned because it can't reach a majority, so no correctness violation occurs even though it kept acting as "leader" locally.

**What happens when a follower has conflicting log entries?**
The leader's AppendEntries RPC includes the index/term of the entry immediately preceding the new ones. If the follower's log doesn't contain a matching entry at that index/term, it rejects the RPC. The leader then decrements `nextIndex` for that follower and retries with an earlier point, until it finds a matching point — then the follower deletes any conflicting entries after that point and appends the leader's entries.

**Why is nextIndex needed?**
It's the leader's best guess of the next log index it should send to a given follower — it lets the leader efficiently probe backward to find the point of agreement after a follower falls behind or has conflicting entries, without resending the entire log every time.

**What does matchIndex represent?**
The highest log index the leader *knows* (has been positively acknowledged) to be replicated on a given follower — used by the leader to determine what's safe to commit (an entry is committed once `matchIndex` for a majority of nodes, including the leader itself, is ≥ that index).

**When is an entry committed?**
When it's stored on a majority of nodes *and* the leader has verified this (via matchIndex) *and* — critically — the leader only commits entries from its *current* term directly; entries from previous terms get committed indirectly, only once a current-term entry is committed (this is the "Leader Completeness"-preserving detail in the Raft paper, added specifically to avoid a subtle safety violation).

**Why isn't "replicated to one follower" enough?**
Because a single follower can itself fail or be partitioned away right after acknowledging, so relying on one copy provides no durability guarantee against any single node failure — you need overlap-guaranteeing majority replication so that any future leader (who must also win a majority) is guaranteed to have seen the entry.

**What happens when the leader crashes?**
Followers stop receiving heartbeats, their election timeouts fire, and a new election begins. Any entries the old leader had only locally (not yet replicated to a majority) may be lost — this is expected and safe, since such entries were never actually committed/acknowledged to the client.

**What happens if the network partitions the leader?**
If the leader ends up in the minority partition, it can no longer get majority acks, so it can't commit anything new — effectively it becomes a leader-in-name-only. The majority partition, after election timeouts fire, elects a new leader and continues making progress.

**Can there be two leaders?**
Yes — but only across *different terms*, and only one of them (the one in the majority partition, with the current term) can actually commit entries. Never two leaders in the *same* term (see above — quorum math forbids it).

**Under what circumstances (two leaders)?**
Classically: a network partition where the old leader is stuck in the minority side (still thinks it's leader, keeps sending heartbeats within its side) while the majority side times out and elects a new leader with a higher term.

**Why doesn't that necessarily violate Raft safety?**
Because safety in Raft is about the *committed log*, not about "how many nodes think they're leader." The stale leader can't get majority acknowledgment for anything, so nothing it does can become committed — the state machine safety guarantee is preserved because "committed" is defined in terms of majority durability, which only the legitimate majority-partition leader can achieve.

**What happens when a partition heals?**
The stale leader receives an RPC (or response) carrying a higher term, immediately reverts to follower, and its own log — if it has uncommitted entries the new leader doesn't share — gets those entries overwritten/truncated by subsequent AppendEntries from the current leader, per the Log Matching property enforcement.

**Why are terms persisted?**
So that after a crash and restart, a node doesn't forget what term it was last participating in — otherwise it could accidentally vote twice in the same term across a restart (once before crashing, once after, having "forgotten" the first vote), which would break the one-vote-per-term invariant that guarantees at most one leader per term.

**Why must votedFor be persisted?**
Same underlying reason as term persistence: if a node votes for candidate A, crashes, restarts, and (having forgotten) votes for candidate B in the *same* term, you can end up with two leaders in one term — directly violating Election Safety. Persisting `votedFor` alongside `currentTerm` before responding to a vote request is what prevents this.

**What happens during crash recovery?**
The node reloads `currentTerm`, `votedFor`, and its persisted log from disk (only entries that were actually fsynced are guaranteed present — this is exactly the torn-write scenario your simulated disk should model), resets all volatile state (`commitIndex`, `lastApplied`, and if it was a leader, its `nextIndex`/`matchIndex` arrays) to initial values, and rejoins as a follower, waiting for either a heartbeat from a current leader or its own election timeout.

### 8.3 DST-Specific Questions

**What sources of nondeterminism does your simulator control, and what does it deliberately leave out (and why)?**
*Controlled:* wall-clock time (virtualized), network delivery order/timing/loss, disk write durability/torn-writes, and any use of randomness in the protocol itself (e.g., randomized election timeouts must be seeded from the same PRNG, not `rand::random()`).
*Deliberately left out (and why):* typically CPU/memory-level nondeterminism (cache effects, floating-point nondeterminism across platforms) — out of scope because it doesn't affect protocol-level correctness the way message/disk nondeterminism does; and Byzantine faults (nodes lying/acting maliciously) — out of scope because Raft itself isn't designed to tolerate Byzantine behavior, so testing for it wouldn't validate anything meaningful about *this* protocol.

**How do you simulate a torn write, and what invariant does your system need to hold to survive it?**
Model the disk write as: the sim disk applies only the first *K* bytes of a write (K chosen by the seeded PRNG, from 0 to full length) before a simulated crash, then on "recovery," only bytes up to the last confirmed `fsync()` are visible. The invariant your system needs: **every persisted record must be individually verifiable/self-consistent** (e.g., checksummed, or written in an order such that a torn write only ever loses the *tail* of the log, never corrupts an already-durable earlier entry) — this is why real systems use append-only logs with per-record checksums rather than in-place overwrites.

**If your DST harness finds a bug, how do you turn the seed into a minimal reproducing test case rather than a 10,000-event trace?**
Delta-debugging: treat the event trace as a sequence, and binary-search-style remove chunks of events, re-running the simulation on the reduced trace each time, keeping the removal only if the invariant violation still reproduces. Repeat until no further events can be removed without the bug disappearing — the surviving events are your minimal reproducer. (This is the same algorithm class as `git bisect` or C-Reduce, applied to an event log instead of commits/source code.)

**Why is DST fundamentally different from, and complementary to, Jepsen-style testing?**
Jepsen is *black-box*: it runs your actual production binary on real (or realistically emulated) machines and injects real faults (killing processes, using `iptables` to partition real network traffic), then checks the observed history against a consistency model using a checker like Elle. It's high-fidelity to production but samples the fault/schedule space sparsely and non-reproducibly (a Jepsen failure might not repeat on the next run). DST is *white-box*: it requires you to architect the system's I/O to be simulator-controllable, but in exchange gives you exhaustive-ish, cheap (single-machine, single-thread), and perfectly reproducible exploration of the schedule space. They're complementary — DST catches protocol-level logic bugs cheaply and reproducibly during development; Jepsen catches bugs in the *real* deployed system (including things DST can't model, like actual OS/hardware quirks) closer to production.

---

## 9. Suggested Study Order

1. Read the Raft paper (Ongaro & Ousterhout) end-to-end once, ignoring implementation for now — just to internalize *why* each mechanism exists.
2. Build Phase 1 (I/O traits + trivial KV store) — get comfortable with the "no ambient state" discipline.
3. Build Phase 2 (simulator core) in isolation, with a fake toy protocol (not Raft yet) — verify determinism first, before adding protocol complexity.
4. Build Phase 3 (Raft on top of the simulator) — get the happy path (no faults) fully working and passing.
5. Add Phase 4 (fault injection) incrementally: network faults first, then disk faults — each one should immediately let you *manually* construct a scenario and watch Raft handle (or fail to handle) it.
6. Add Phase 5 (invariant checker) — validate it by deliberately breaking something (e.g., comment out `votedFor` persistence) and confirming the checker catches it. This is your "proof the checker works" step.
7. Only then move to Phase 6–9 (fuzzing at scale, real bug discovery, minimization, production polish) — this is where your actual differentiating artifact (the found bug + minimized repro) comes from.
