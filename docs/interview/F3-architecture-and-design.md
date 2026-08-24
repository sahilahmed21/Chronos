# F3 — Architecture and design decisions

**Audience:** zero Rust. Defend Chronos in a system-design / architecture round. Every claim below is from `docs/02-architecture.md` (constitution) or shipped v1 behavior. Do not invent features.

**Hard honesty:** Chronos is **co-designed deterministic simulation testing (DST)** for a minimal Raft KV you control. `chronos-node` v1 is a **single-node FileDisk demo**, **not** a three-process TCP HA cluster. Jepsen and Antithesis are **out of v1**, not competitors you secretly shipped.

---

## 1. One-sentence architecture

**Hexagonal functional core:** the protocol is `step(State, Event) → Vec<Effect>`; `chronos-sim` and `chronos-node` are interpreters (adapters); sim and node never depend on each other.

---

## 2. Hexagonal map (draw this)

```text
                    ┌─────────────────────────────────────┐
                    │           composition roots          │
                    │   chronos-sim          chronos-node  │
                    └──────────────┬──────────────┬────────┘
                                   │              │
              inbound adapters     │              │ inbound adapters
              (sim scheduler,      │              │ (CLI / FileDisk;
               fault injector,     │              │  TCP later — stub now)
               simulated clients)  │              │
                                   ▼              ▼
                    ┌─────────────────────────────────────┐
                    │     inbound port = Node::step       │
                    │   (State, Event) -> (State, Effects)│
                    │         chronos-protocol            │
                    └──────────────────┬──────────────────┘
                                       │ effects = outbound port
                                       ▼
                    ┌─────────────────────────────────────┐
                    │     outbound adapters (interpreters) │
                    │  sim: queued net/disk/timer/entropy  │
                    │  node: files / SystemTime / getrandom│
                    └─────────────────────────────────────┘
```

**What “hexagonal” means here (no buzzword fog):**

- The **use case** (Raft + KV) does **not call** outbound ports.
- It **returns** outbound work as **effects**.
- Interpreters apply effects and later feed **completions** back as **events**.

That is the FoundationDB `INetwork` / `IAsyncFile` split **without** pretending Raft is an async Rust service.

### Crate rules

| Crate | Owns | Must not own |
|-------|------|----------------|
| `chronos-protocol` | Raft, KV apply, WAL codec, Event/Effect | sockets, files, Instant, RNG, HashMap |
| `chronos-sim` | Scheduler, PRNG, sim net/disk, checkers, fuzz, minify | dependency on node |
| `chronos-node` | FileDisk, OS time/entropy for timers | a forked protocol |

Enforcement: clippy disallowed types + CI grep (D16). Cargo alone cannot stop `use std::fs` inside protocol.

---

## 3. Why not Tokio / MadSim / Turmoil under the protocol (D2)

**Locked decision:** own a single-thread scheduler in `chronos-sim`.

**Rejected:** Tokio + MadSim or Turmoil under the protocol.

**Why the alternative bites:**

| Pain | Plain English |
|------|----------------|
| Hides the step function | Interviewers (and you) cannot audit `event → effects` if the runtime owns wakes/sleeps. |
| Patch tax | MadSim-style paths need `cfg madsim`, getrandom patches, Instant shims. |
| Windows-hostile | libc / OS shims fight a Windows+Linux determinism story. |
| Still leaks HashMap | Replacing the runtime does not fix random HashMap iteration in protocol or sim. |
| Wrong product shape | Chronos is greenfield. Optimizing for “looks like Tokio” fights a protocol that should not be async. |

**Speakable line:** “We own the event loop so the protocol stays a pure step function and the sim owns time, net, disk, and entropy.”

---

## 4. Why effects + completions, not a sync `Disk` trait (D3)

**Rejected API:** `Disk::write() -> Result` / sync `Network::send()` called *inside* Raft.

**That API cannot express:**

1. Fsync still in flight while a vote RPC is on the wire.
2. Disk delay of tens of ms during an election.
3. Crash between “buffer updated” and “durable watermark advanced.”
4. Reordering disk **completions** vs message delivery.

TigerBeetle and Dropbox Trinity intercept **completions**. Chronos starts there: submit now, complete later as `Event::IoComplete`.

**Narrow persist-before-send (not a blanket ban):**

| Legal | Illegal |
|-------|---------|
| Leader `Append` + `Send AppendEntries` in the same effect batch | `Send RequestVote` before Meta Fsync Ok |
| | `Send` granting `RequestVoteResp` before Meta Fsync Ok |
| | `Send` successful AppendEntries response before log Fsync Ok |

Leader must not count itself toward `matchIndex` / commit until its append’s Fsync completes Ok.

**Blanket “no Send with IoSubmit”** is wrong: it rejects legal leader replication (D3 rejected alternative).

---

## 5. Locked decisions D1–D17 (interviewer-friendly why)

Memorize the **Choice** and one **Why**. Rejected alternatives are in the constitution table.

| ID | Choice | Rejected | Interviewer-friendly why |
|----|--------|----------|---------------------------|
| **D1** | Rust | Go, Zig, C++ | Spec is trait-shaped; interviewer expects Rust DST; Zig is TigerBeetle’s world, not ours; Go map iteration is also nondeterministic. |
| **D2** | Own single-thread scheduler | Tokio + MadSim/Turmoil | Keeps `step` visible; avoids cfg/getrandom/Windows pain; HashMap still leaks under mocked runtimes. |
| **D3** | Effects + completions; persist-before-send/ack only for votes and AE-success | Sync Disk/Network; blanket no-Send-with-IoSubmit | Sync cannot interleave crash with in-flight I/O; blanket rule breaks legal AppendEntries. |
| **D4** | Tickless `u64` ns; heap key `(time, global seq)` | Integer ticks + shuffle; per-node seq; PRNG among ties | TigerBeetle already paid the tick rewrite; per-node seq biases by NodeId when cluster size changes. |
| **D5** | Three crates | One crate “for now” | Sim will import `std::net` “just in a test” and poison the protocol. |
| **D6** | `BTreeMap` / `BTreeSet` / `Vec` in protocol **and** sim | Deterministic hasher; ban HashMap only in protocol | Sim iteration (net, disk, check, trace) is what digests hash; a hasher is easy to forget. |
| **D7** | Get via log in v1 | Leader memory reads | Deposed leaders + no read-index → false linearizability fails → fake “DST found a bug.” |
| **D8** | Idempotency keys on every client op; table is SM state | Retry without keys; table as side cache | Duplicate Puts look like Raft bugs; divergent tables break State Machine Safety. |
| **D9** | Checksummed append-only WAL from day one; torn suffix **past** `durable_len` | Raw buffer; tear by shrinking `durable_len` | Format change later hurts; shrinking `durable_len` **is** an fsync-lie (D10). |
| **D10** | Fsync-lie **off** by default | Always-on firmware lies | “Discovers” that paper Raft is unsafe (known). Pollutes the Phase-7 artifact. |
| **D11** | Static membership, no snapshots | Full Raft | Membership is a second protocol; bound sim length instead of inventing compaction. |
| **D12** | Two checkers; apply **inside** `step` | Invariants only; optional `Effect::Apply` | KV can be wrong while Raft looks perfect; optional Apply splits the oracle. |
| **D13** | SHA-256 of encoded `TraceRecord`s | Eyeball logs; `std::hash::Hasher` | Hasher is toolchain-unstable; host fields make Windows ≠ Linux. |
| **D14** | LE `[len][crc32(len\|\|payload)][payload]` | JSON / bincode; CRC over payload only | JSON key order; torn `len` is silent if CRC ignores length bytes. |
| **D15** | `ArmTimer { kind }`; harness owns durations and `now` | `SetTimer { at }`; `step(node, now, event)` | Protocol must have no clock and no RNG; absolute `at` forces both. |
| **D16** | Clippy disallowed-types + CI grep | “Cargo will catch it” | Cargo only sees crate deps; `use std::fs` still compiles. |
| **D17** | WAL `Meta` + `Entry` append-only; Appends in emission order | In-place term/votedFor header; reorder Writes | In-place fights torn-tail; reordered Appends scramble CRC boundaries. |

---

## 6. Disk model (speak carefully)

Per simulated node:

| Field | Meaning |
|-------|---------|
| `bytes` | What the process’s buffer currently contains |
| `durable_len` | Prefix length of the last fsync the sim decided **succeeded** |
| in-flight ops | Append/Fsync with scheduled completion times |

### Rules

1. **Append** mutates `bytes` immediately, in **emission order**, and does **not** move `durable_len`.
2. **Fsync** snapshots `sync_len = bytes.len()` at submit. On Ok: `durable_len = max(durable_len, sync_len)`. A write after that submit cannot sneak into that fsync.
3. Completions of Append, Fsync, and Send **may** delay/reorder relative to each other (the interleaving under test).
4. Two Appends to the same node’s WAL are **never** reordered relative to emission (CRC boundaries).

### Crash

1. Keep `bytes[0..durable_len]`.
2. Replace the suffix past `durable_len` with a seed-chosen torn prefix of in-flight Appends (may end mid-record).
3. Drop in-flight I/O; deliver Recover.

### Recover

Scan records from 0; stop on bad header / CRC fail; truncate to last good record; replay Meta/Entry into Raft state. Last Meta wins for `(currentTerm, votedFor)`.

### Torn ≠ fsync-lie (D9 / D10)

| Knob | What it does | Default |
|------|----------------|---------|
| Torn suffix | Mutates only `bytes[durable_len..]` | **on** |
| Fsync-lie | Reports Ok **without** advancing `durable_len` | **off** |
| Fail fsync with Err | Caller must not send vote/ack | on |

**Speakable line:** “Torn tail is honest media truncated mid-record past a real fsync watermark. Fsync-lie is lying firmware. Paper Raft assumes stable storage is stable; we model the lie but keep it off so Phase 7 is not a known limitation.”

---

## 7. Network model

- On `Send`: harness assigns `MsgId`; `delivery_at = now + delay_ns` (PRNG, possibly 0).
- Drop / duplicate from policy or recorded schedule tokens.
- Partition = adjacency matrix (symmetric by default; asymmetric is a flag).
- Reorder **emerges** from per-message random delay (no special reorder queue in v1).
- Messages already on the wire **survive** sender crash (Raft terms make stale RPCs safe).
- Disk `IoComplete`s do **not** survive crash of that node.

---

## 8. Time, entropy, collections

| Concern | Rule |
|---------|------|
| Time | Tickless `Timestamp(u64)` nanoseconds; `now` = popped event time |
| Heap key | `(time, global seq)` — one global counter, never per-node |
| Entropy | One seeded RNG in sim for delays, drops, client targeting, **election** durations |
| Heartbeat duration | Fixed config + recorded jitter (must be in `ReplayBook`) |
| Protocol timers | Only `ArmTimer { kind: Election \| Heartbeat }` — no `now`, no RNG in protocol |
| Collections | `BTreeMap` / `BTreeSet` / `Vec` in protocol **and** sim |
| Numbers | No `f64` in protocol or scheduled times |
| Encoding | Hand-rolled little-endian; identical Windows/Linux |

### Why BTreeMap, not HashMap (D6)

Default Rust `HashMap` iteration order is randomly seeded from the OS. Same seed would not yield the same sim iteration order → digests diverge on Windows vs Linux and across runs. A “fixed hasher” is easy to forget in one net/disk/check/trace path. Ban HashMap in **both** protocol and sim.

---

## 9. Raft + KV choices interviewers poke

### Get via log (D7)

Serving Get from leader memory without read-index fails linearizability under a deposed leader and farms fake DST bugs. v1 puts **Get in the log**. Read-index is a later extension; the checker stays the same.

### Figure 8 / no-op

A leader must not commit previous-term entries by counting replicas alone. Only committing a **current-term** entry (by majority) commits prior entries indirectly (Log Matching). New leaders append a **no-op** (or equivalent current-term entry) before committing old-term entries. Figure 8 is a **checker case**, not a blog comment.

### Idempotency (D8)

Every client op carries `(client_id, request_id)`. The idempotency table lives in `kv/machine.rs`, updates only on apply, and is covered by State Machine Safety. Unbounded in v1; bound sim length instead of inventing snapshots.

### Static membership (D11)

No join/leave, no snapshots/compaction in v1.

### Apply inside step (D12)

No `Effect::Apply`. Harness snapshots `last_applied` after step for checkers/trace.

---

## 10. Checkers — white box vs black box

Run white-box **after world events** (cheap) and black-box **at end** (history). Two independent oracles.

### White-box (Raft + engineering)

Inspect in-memory logs and roles:

1. Election Safety  
2. Leader Append-Only  
3. Log Matching  
4. Leader Completeness  
5. State Machine Safety (includes KV idempotency table as applied state)

Plus engineering:

- Persist-before-send / persist-before-ack (`durable` cookie vs outgoing vote/grant/AE-success)
- `commitIndex` never decreases
- Leader only counts majority for entries with `entry.term == currentTerm`
- Leader does not advance `matchIndex[self]` until covering Fsync Ok

### Black-box (KV linearizability)

Simulated clients record invoke/complete with virtual `(time, seq)`. Spec = sequential map. Porcupine-style search.

| Result class | Meaning |
|--------------|---------|
| Definite miss (`NotLeader`, `Invalid`) | Op did not apply |
| Unknown (`Io`, incomplete after possible apply) | Try **both** apply and skip |
| Unmatched Reply | Recorder error, not silent drop |
| Capacity overflow | Not a Linearizability verdict |

Do **not** check linearizability by comparing all KV maps every tick — partitions legally diverge on uncommitted data.

### Liveness (optional flag)

If a majority stays connected for a long window with no further crashes/partitions, a leader exists and a write commits. If the premise is false, the implication is vacuously true — do not fail. Never enable under permanent partitions.

---

## 11. SHA-256 traces (D13)

Trace hash = **SHA-256** of the concatenation of little-endian encoded `TraceRecord`s.

**Must not include:** wall clock, hostname, path, `std::hash::Hasher`.

**Contract:** same seed + same commit ⇒ same digest on Windows and Linux. CI runs a seed twice and asserts digest equality.

---

## 12. Fsync-lie off (D10) — one paragraph you can recite

Protocol-Aware Recovery research shows vanilla Raft is unsafe or unavailable under some storage faults (including “fsync succeeded but bytes never hit media”). Chronos **models** fsync-lie as a knob, default **off**. Turning it on in the default swarm would “find” that paper Raft is unsafe — a **known limitation**, not a Phase-7 win. Torn tails past `durable_len` are a different, default-on fault.

---

## 13. Honest comparison: FoundationDB / TigerBeetle / Jepsen / Antithesis

Chronos is the **co-designed** kind: the runtime exists so the protocol can be a function.

| System | What it is | Relation to Chronos |
|--------|------------|---------------------|
| **FoundationDB sim** | Simulator-first DB; single-threaded pseudo-concurrency; simulated `INetwork`/`IAsyncFile`; swarm + buggify; determinism self-check | **Inspiration.** Same *idea*: own interfaces, seed → replay. Chronos scopes that idea to a minimal Raft KV end-to-end. |
| **TigerBeetle VOPR** | Production replica code in-process; stubbed clock/net/disk; tickless ns time; safety vs liveness modes | **Inspiration** for tickless time and “production code in the sim.” Chronos is not a port of VOPR. |
| **Jepsen** | Black-box testing of **real** deploys against real kernels/NICs; sparse, often non-replayable | **Complement later**, not a substitute, **out of v1**. Catches “our sim misunderstood the OS.” |
| **Antithesis / Hermit** | Deterministic **hypervisor** over unmodified binaries (syscalls/clocks/scheduling) | Right tool when you **cannot** control the I/O boundary. Chronos *can*. Hypervisor DST is out of scope; not shipped. |
| **MadSim / Turmoil** | DST by replacing Tokio runtime for existing async apps | **Rejected under protocol** (D2). |

**Speakable closer (from architecture):**  
“If you want full-system DST of an unmodified binary, that is Antithesis/Hermit. If you want real kernel and NIC bugs, that is Jepsen/sinkhole. Chronos is co-designed DST: like FoundationDB and TigerBeetle, the runtime exists so the protocol can be a function. That is the point of the exercise.”

---

## 14. Production honesty (do not overclaim)

| Claim | Truth |
|-------|--------|
| Same `step` under sim and node | Yes |
| Multi-node TCP HA cluster in v1 | **No** — node is single-node FileDisk demo; `net.rs` is a stub |
| Fsync-lie default-on | **No** |
| Wild PROTOCOL Raft safety bug from swarm | **No** — PLANTED ElectionSafety is the harness proof; seed 281 closed as CHECKER (+ harness heartbeat recording) |
| Membership / snapshots / Byzantine | Non-goals |

---

## 15. Whiteboard sequence — 15-minute design interview

Use this as a timed agenda. Practice drawing while talking.

### Minute 0–2 — Problem frame

- Schedule-dependent distributed bugs (crash between append and fsync; delayed votes; partitions).
- Unit tests check functions; we need to check **schedules**.
- Goal: deterministic fuzz → replayable seed + SHA-256 digest + minimized fault schedule.

### Minute 2–5 — Hexagon + step API

Draw the three-crate diagram (§2).

Write:

```text
fn step(node, event) -> Vec<Effect>
```

Events: TimerFired, MessageReceived, IoComplete, Recover, ClientRequest.  
Effects: Send, IoSubmit(Append|Fsync), ArmTimer, Reply.

Say: protocol has no Instant, no RNG, no HashMap, no sockets.

### Minute 5–8 — Disk + persist-before-send

Draw:

```text
bytes:     [========durable========|....torn?....]
            0                  durable_len      len
```

Sequence for election:

```text
ElectionTimeout → Append Meta + Fsync → (later) IoComplete Ok → Send RequestVote
```

Contrast legal leader: Append Entry + Send AppendEntries same batch; self commit waits for Fsync.

### Minute 8–11 — Checkers + Get-via-log + Figure 8

- White-box five Raft props + persist cookies.
- Black-box linearizability; Io = unknown.
- Get via log (D7); no-op on new leadership (Figure 8).
- Fsync-lie off (D10).

### Minute 11–14 — Product loop

```text
seed → swarm_plan → drain_horizon → checkers
  fail → fail.trace + digest
  replay → verify
  minify → delta-debug FAULT schedule
           Recorded (full) vs Defaults (subsets)
```

Mention PLANTED skip_vote_persist as harness proof; never enable in swarm.

### Minute 14–15 — Scope + invite

Non-goals: membership, snapshots, Jepsen, Antithesis, multi-process TCP HA.  
Invite: “Happy to go deeper on seed-281 triage or the WAL CRC layout.”

---

## 16. Flash answers (30 seconds each)

**Why three crates?**  
So sim cannot import `std::net` into the protocol by accident (D5).

**Why not sync Disk?**  
Cannot crash between buffer update and durable watermark or reorder completions (D3).

**Why BTree everywhere in sim too?**  
Digest hashes iteration order; hasher forgets happen (D6).

**Why Get through the log?**  
Leader-memory reads without read-index produce fake linearizability failures (D7).

**Why fsync-lie off?**  
Would rediscover a known paper-Raft limitation and pollute the finding portfolio (D10).

**Is chronos-node a cluster?**  
No — single-node FileDisk demo; TCP stub only.

---

## 17. Read next

- File-level flows A–F → [F2](F2-files-and-code-flow.md)
- Rust for this project → [F4](F4-rust-for-chronos.md)
- War stories → [F5](F5-problems-and-solutions.md)
- Question bank → [F6](F6-interview-question-bank.md)
- Constitution (wins conflicts) → `docs/02-architecture.md`
