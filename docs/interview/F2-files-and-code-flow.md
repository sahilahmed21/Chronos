# F2 — File map and code flows

**Audience:** zero Rust. After this file you can name which crate owns what, open the right path, and narrate Put / election / crash / swarm / minify / planted ElectionSafety without inventing features.

**Source of truth:** `docs/02-architecture.md` (constitution) + the shipped crates under `crates/`. If something is not in the tree or the architecture doc, do not claim it.

**Hard honesty:** `chronos-node` in v1 is a **single-process FileDisk WAL demo** with a **net stub**. It is **not** a three-process TCP Raft cluster.

---

## 0. Plain-English map (no Rust jargon)

Think of Chronos as three boxes:

| Box | English name | Job |
|-----|--------------|-----|
| `chronos-protocol` | The brain | Takes one event, updates one node’s memory, returns a list of *requests* (effects). Never talks to disk/network/clock. |
| `chronos-sim` | The lab | Fake time, fake net, fake disk, fake clients, checkers, fuzz, minify. Drives many “nodes” in **one OS thread**. |
| `chronos-node` | The demo interpreter | Same brain, real file WAL for one node. OS time/files only here. |

The brain’s only public transition is:

```text
step(node, event) → list of effects
```

- **Event** = something the world does *to* a node (timer fired, message arrived, disk finished, client asked, recover after crash).
- **Effect** = something the node asks *of* the world (send a message, submit Append/Fsync, arm a timer, reply to a client).

Sim and node **interpret** effects. They never depend on each other. Both depend only on protocol.

---

## 1. Workspace layout

```text
Chronos/
  Cargo.toml                 # workspace lists the three crates
  clippy.toml                # bans HashMap / Instant / fs / net / thread where required
  crates/
    chronos-protocol/        # pure step — NO I/O
    chronos-sim/             # deterministic world + CLI binary
    chronos-node/            # production-style FileDisk demo binary
  docs/                      # architecture, roadmap, bugs, interview/
```

**Cargo dependency rule (memorize):**

```text
chronos-sim  ──depends──►  chronos-protocol
chronos-node ──depends──►  chronos-protocol
chronos-sim  ✗  chronos-node   (never)
chronos-node ✗  chronos-sim   (never)
```

Cargo enforces that graph. Cargo does **not** stop `use std::fs` inside protocol — clippy + CI grep do (architecture D16).

---

## 2. `chronos-protocol` — file map

Paths under `crates/chronos-protocol/`.

| Path | What it is (plain English) |
|------|----------------------------|
| `src/lib.rs` | Public surface / re-exports. Documents: no I/O, no clock, no RNG. |
| `src/types.rs` | IDs and numbers: `NodeId`, `Term`, `Index`, `Timestamp` (u64 nanoseconds), `IoId { incarnation, local }`, `TimerId`, `ClientId`, `RequestId`, commands. |
| `src/event.rs` | `Event` enum (world → node): `TimerFired`, `MessageReceived`, `IoComplete`, `Recover`, `ClientRequest`. Messages and client requests live here. |
| `src/effect.rs` | `Effect` enum (node → world): `Send`, `IoSubmit` (`Append` / `Fsync`), `ArmTimer` / `CancelTimer`, `Reply`. |
| `src/codec.rs` | Hand-rolled little-endian binary encoding for messages / deterministic bytes. No JSON. No serde. |
| `src/wal/record.rs` | WAL on-disk shape: length-prefixed + CRC records; `Meta` and `Entry` payloads; scan/truncate helpers. |
| `src/kv/machine.rs` | Apply one committed log entry to the KV map; idempotency table (`client_id` + `request_id`) is **state-machine state**. |
| `src/raft/mod.rs` | Raft submodule wiring. |
| `src/raft/state.rs` | `Node` struct: role, term, votedFor, log, commit/applied, cookies, in-flight I/O, optional test hook `skip_vote_persist`. |
| `src/raft/log.rs` | Append-only Raft log: append, truncate, term-at-index, matching helpers. |
| `src/raft/election.rs` | Election timeout → Meta+Fsync; RequestVote / grants; become leader; **persist-before-send** for votes. |
| `src/raft/replication.rs` | Client Put/Get via log; AppendEntries; `maybe_commit`; apply inside step; `replicate_all`; Figure 8 / no-op path. |
| `src/raft/step.rs` | **`step(node, event) → Vec<Effect>`** — the only state transition function. Dispatches to election / replication / recover / IoComplete. |

### How `step` dispatches (interview cheat sheet)

| Incoming `Event` | Goes to |
|------------------|---------|
| `TimerFired` (election) | `election::on_election_timeout` |
| `TimerFired` (heartbeat) | `replication::on_heartbeat` |
| `MessageReceived` RequestVote / RequestVoteResp | `election::…` |
| `MessageReceived` AppendEntries / AppendEntriesResp | `replication::…` |
| `ClientRequest` | `replication::client_request` |
| `IoComplete` | Advances durable cookie / may send pending votes or AE acks / may commit+apply |
| `Recover` | Rebuild term / votedFor / log from durable WAL bytes; reset volatile |

**Apply is inside `step`.** There is no `Effect::Apply`. When `commitIndex` advances, apply runs before effects return to the harness.

---

## 3. `chronos-sim` — file map

Paths under `crates/chronos-sim/`.

| Path | What it is |
|------|------------|
| `src/lib.rs` | Module exports (`Cluster`, `DelayBind`, `ReplayBook`, fuzz helpers, …). |
| `src/rng.rs` | Single seeded PRNG for a run. Integer delays/bools only. Protocol never calls this. |
| `src/scheduler.rs` | Min-heap of world events keyed by `(Timestamp, global seq)`. Tickless nanoseconds. |
| `src/net.rs` | On `Send`: assign `MsgId`, delay, drop/dup/partition. Delivery becomes `MessageReceived`. |
| `src/disk.rs` | Per-node `bytes` + `durable_len` + in-flight ops. Append/Fsync/crash torn/Recover scan. |
| `src/cluster.rs` | N logical nodes; interpret effects; `drain` / `drain_horizon`; observe schedule; `ReplayBook`; `set_skip_vote_persist` (tests). |
| `src/check.rs` | White-box Raft safety + engineering invariants (persist-before-send/ack, …). |
| `src/history.rs` | Client invoke/complete history → linearizability search. `Io` is **unknown** (apply-or-skip), not a definite miss. |
| `src/fuzz.rs` | `swarm_plan(seed)`, `run_seed`, fail files, coverage, replay verify. |
| `src/minify.rs` | Atomize fault schedule; delta-debug; `DelayBind::Recorded` vs `Defaults`. |
| `src/trace.rs` | Structured `TraceRecord`s → **SHA-256** digest (no host clock/paths). |
| `src/main.rs` | CLI: `--seed` / `--seeds` / `--replay` / `--minify` / `--coverage`. |
| `tests/properties.rs` | Planted `skip_vote_persist`; happy path; linearizability; liveness. |
| `tests/scenarios.rs` | Named fault scenarios (partition, delay, crash, …). |

### Two drain modes (landmine)

| API | Stops when | Used by |
|-----|------------|---------|
| `Cluster::drain()` | Live leader **and** heap has no **non-timer** events | Most unit / scenario tests |
| `Cluster::drain_horizon()` | Empty heap / past `max_ns` / check fail | Swarm, minify |

Do **not** put horizon drain back on idle unit tests — heartbeats burn `max_ns` and hang CI.

### `ReplayBook` fields (what must be recorded)

So minify/replay does not re-roll the PRNG for delays:

- `send_delay`
- `io_delay`
- `election`
- `heartbeat` (same `(node, life)` key style as election; required so Recorded matches live)

Also observed on the schedule: drop/dup delivery tokens, crash torn extras, etc.

---

## 4. `chronos-node` — file map (v1 reality)

Paths under `crates/chronos-node/`.

| Path | What it is |
|------|------------|
| `src/main.rs` | Single-node demo: open WAL file → `FileDisk` → `drive` loop calling `Node::step` + disk completions for Recover, election timer, Put, Get. |
| `src/disk.rs` | `FileDisk` — real file Append/Fsync interpreter for the same WAL record format. |
| `src/timer.rs` | OS timer helpers for the node path. |
| `src/net.rs` | **Stub / placeholder** for later length-prefixed TCP. **Not** wired as a 3-process cluster in shipped v1. |

**Say in interviews:** same protocol crate; production interpreter path exists; **v1 is not multi-node TCP HA**.

---

## 5. Docs / CI interviewers may ask for

| Path | Role |
|------|------|
| `docs/02-architecture.md` | Constitution; D1–D17 win conflicts |
| `docs/roadmap/goals.md` | G0–G11 done definition |
| `docs/bugs/2026-08-24-planted-skip-vote-persist/` | PLANTED ElectionSafety pack |
| `docs/bugs/HUNT-2026-08-24.md` / `CLOSE-281.md` | Seed 281 → HARNESS then CHECKER |
| `.github/workflows/ci.yml` | fmt, release tests, clippy, determinism gates, swarm 32/1000 |

---

## 6. Flow A — Client Put on leader (end-to-end, numbered)

Happy path: leader already elected; client Put reaches that leader.

1. **World inject.** Harness schedules a client op targeting the leader (sim: `inject_client` / swarm client event). History records **invoke** `(client, request)`.
2. **Deliver as event.** Cluster applies the world event → `Event::ClientRequest { req }` with `Cmd::Put { key, value }` plus idempotency ids.
3. **`step` on leader.** `replication::client_request`:
   - If not leader → `Effect::Reply` `NotLeader` (definite miss for linearizability) and stop.
   - Else append a log `Entry` (opaque client payload + ids) to the in-memory Raft log.
4. **Persist effects (same batch).** Emit `IoSubmit(Append …)` then `IoSubmit(Fsync)` covering that log prefix. Cookie tracks what the persist covers.
5. **Replicate early (legal).** Same batch may also `replicate_all` → `Effect::Send` `AppendEntries` to peers. Architecture D3: **Send with IoSubmit is legal for log replication**. Leader must **not** count self toward `matchIndex` / commit until Fsync completes Ok.
6. **Sim disk Append.** Interpreter applies Append **immediately** to that node’s `bytes` **in emission order**. Does **not** move `durable_len`. Enqueues `IoComplete(Append)` for later.
7. **Sim disk Fsync.** Submitted against length-at-submit snapshot. Later completion may Ok or Err; delay/reorder of completions is intentional.
8. **`IoComplete(Fsync Ok)` → `step` again.** Durable cookie advances; leader’s `matchIndex[self]` may advance; `maybe_commit` runs.
9. **`maybe_commit`.** Only current-term entries count by majority (Figure 8). If `commitIndex` advances:
   - **apply** committed entries **inside** `step` (KV machine / idempotency table);
   - call **`replicate_all` again** so followers learn the new `leaderCommit` even if idle `drain()` would stop before the next heartbeat.
10. **Client reply.** When the Put’s entry is applied, emit `Effect::Reply` Ok (or stored idempotent response). History **complete**.
11. **Followers (parallel track).** They receive AppendEntries, persist their own Append+Fsync, and only Send successful AE responses after Fsync Ok (**persist-before-ack**). Their apply also happens inside `step` when commit advances.

**Get:** also a log entry (D7), not a leader-memory read. Apply returns value / NotFound from the state machine.

---

## 7. Flow B — Election timeout → Meta → VoteFsync Ok → RequestVote (persist-before-send)

1. **Election timer fires.** World delivers `Event::TimerFired` with the election timer id to a follower/candidate.
2. **`election::on_election_timeout`.** Bump `currentTerm`, set `votedFor = self`, role → Candidate, clear vote set.
3. **Persist Meta first.** Emit `IoSubmit(Append Meta record)` + `IoSubmit(Fsync)` with work kind **VoteFsync / Campaign**. Cookie covers `(term, votedFor=self, …)`.
4. **No RequestVote yet (honest path).** Same effect batch does **not** include `Send RequestVote` (unless the test-only `skip_vote_persist` hook is on).
5. **Arm election timer again.** So campaigns can retry if votes stall (`ArmTimer { kind: Election }`; harness picks duration from RNG range).
6. **Sim interprets Append/Fsync.** Bytes update; durable_len advances only on Fsync Ok completion.
7. **`IoComplete(Fsync Ok)`.** `step` runs `on_vote_fsync_ok` for Campaign → now emit `Effect::Send { RequestVote, … }` to each peer.
8. **That ordering is the invariant.** Checker compares outgoing vote traffic to the **durable** cookie, not volatile memory. sofa-jraft-class bug: send then persist.
9. **Peers grant path.** On RequestVote, a grant also persists Meta before sending `RequestVoteResp { granted: true }` (persist-before-send for grants). Rejects with lower term can reply without that persist path when appropriate.
10. **Majority → leader.** Become leader; append **no-op** (or equivalent current-term entry) before committing old-term entries; start heartbeats via `replicate_all` / heartbeat timer.

---

## 8. Flow C — Crash → durable_len + torn → Recover CRC scan

1. **World crash.** Harness applies crash for a node (not a `Node` event). Mark node not alive; bump life / I/O incarnation bookkeeping.
2. **Honest durable prefix.** Keep `bytes[0..durable_len]` intact. `durable_len` still means “last successful fsync the sim decided.”
3. **Torn suffix (default fault knob on).** Replace `bytes[durable_len..]` with a seed-chosen prefix of in-flight Appends that had already mutated the buffer (0..=full pending tail). May end **mid-record**. This is **not** an fsync-lie.
4. **Drop in-flight I/O** for that node. Disk completions do **not** survive. (In-flight **network** messages from the sender may still arrive at receivers.)
5. **Discard volatile Node shell.** Memory role/log/commit are gone until rebuild.
6. **World recover.** Deliver `Event::Recover` with whatever the disk still holds after crash mutation.
7. **CRC scan.** From offset 0: parse `[u32 len][u32 crc32][payload]`. Stop at first header that does not fit or whose CRC fails (CRC covers `len || payload`). Truncate `bytes` (and `durable_len` if needed) to end of last good record.
8. **Replay records into Raft/KV durable facts.** Last `Meta` wins for term/votedFor; `Entry` records rebuild the log. Volatile fields reset (`commitIndex` starts at 0 until new leadership/commits).
9. **Incarnation.** New I/O incarnation; old `IoComplete`s ignored so late completions cannot alias post-restart submits.

---

## 9. Flow D — Swarm `run_seed` → `swarm_plan` → `drain_horizon` → fail file

1. **CLI / CI** calls into `fuzz::run_seed(S)` (or `--seeds` loop).
2. **`swarm_plan(S)`.** Seeded PRNG builds a plan: profile calm|brutal, `n ∈ {3,5}`, timeout ranges, delay distributions, fault probabilities, client rate, extras (crashes, partitions, …).
3. **`Cluster::new`** with that config + fresh disks/RNG for the seed.
4. **Bootstrap.** Inject `Recover` for each node; schedule extras from the plan.
5. **`drain_horizon()`.** Loop until empty / past `max_ns` / white-box check fail:
   - Pop earliest `WorldEvent` from heap (`time`, global `seq`).
   - Set virtual `now` to that time.
   - Apply (deliver message / IoComplete / TimerFired / Crash / Partition / Client / …).
   - If it targets a node: `effects = Node::step(event)`.
   - Interpret effects **in emission order** (Append immediate; Fsync/Send/ArmTimer enqueue completions/deliveries with recorded delays).
   - Run white-box checkers after world events.
6. **End of run.** Black-box linearizability on client history; build SHA-256 trace digest; coverage flags.
7. **On CheckFail.** Write `fail-S.trace` (+ planned extras dump). Print seed, digest, first violating check. Exit non-zero.
8. **On success.** Print ok seed + digest. Determinism gate elsewhere: same seed twice ⇒ same digest.

Swarm **never** enables `skip_vote_persist`. That hook is test-only / PLANTED packs.

---

## 10. Flow E — Minify: `DelayBind::Recorded` vs `Defaults`

Goal: shrink the **fault/delivery schedule** (extras / drops / dups / …), not randomly delete Raft internals.

1. **Live observe.** Run the failing seed; require a `CheckFail`. Capture full `ObservedSchedule` / `ReplayBook` (send/io/election/**heartbeat** delays + tokens).
2. **Atomize.** Turn the schedule into atoms suitable for delta-debugging (causal extras; delivery tokens by content hash, not fragile MsgId ordinals).
3. **Verify full schedule under `DelayBind::Recorded`.** Replay pops recorded delay FIFOs so the unfiltered schedule matches live’s **`CheckName`**. If not → **MINIFY MISMATCH** (harness/recording bug — triage HARNESS, do not claim PROTOCOL).
4. **Why Recorded exists.** Without recorded heartbeat jitter (seed 281 lesson), live and Recorded diverge at Heartbeat `TimerFired` times even when extras match.
5. **Delta-debug subsets under `DelayBind::Defaults`.** Subsets must **not** steal another send’s recorded delay. Defaults fall back to config (e.g. bare `heartbeat_ns`, planned ranges) so deleted atoms do not poison FIFO alignment.
6. **Success oracle.** Same failing **`CheckName`**, **not** digest equality (D13 digest is for CI determinism of full runs; minify compares the check).
7. **Output.** Write `fail-S.min` / schedule.min text. CLI minify must **not** plant `skip_vote_persist`.

| Mode | When | Delays |
|------|------|--------|
| `DelayBind::Recorded` | Full observed schedule verify | Pop `ReplayBook` FIFOs |
| `DelayBind::Defaults` | Subset delta-debug | Config fallbacks; empty / default book |

---

## 11. Flow F — Planted `skip_vote_persist` ElectionSafety (outline)

**Purpose:** Harness proof that Election Safety + persist-before-send oracles work. **CLASS=PLANTED.** Not a wild swarm PROTOCOL find. Swarm/CLI never enable the hook.

**Test:** `planted_skip_vote_persist_fails_election_safety` in `crates/chronos-sim/tests/properties.rs`. Pack: `docs/bugs/2026-08-24-planted-skip-vote-persist/`.

Outline (what the scenario *does*, not a second implementation):

1. Build a small static cluster (3 nodes) with scripted delays/timeouts (fixed election/io delays in the test config).
2. **`set_skip_vote_persist(NodeId(0), true)`** — test-only: on election/grant paths, send RequestVote / grant **without** waiting for Meta Fsync Ok.
3. Partition so node 2 is cut from 0 and from 1 initially; recover all three.
4. Cancel elections on 0 and 2; fire election on **1** so a campaign proceeds under partitions.
5. Step until node **0** has **granted** a vote (observable on the wire) — grant happened **without** durable votedFor because of the hook.
6. **Crash node 0** (volatile grant gone; Meta may never have been durable).
7. **Recover node 0**; change partitions (isolate 0–1; heal 0–2); fire election on **2**.
8. Node 0 can vote again in the **same term** for another candidate → two leaders of one term become observable.
9. White-box checker fails **`ElectionSafety`**.
10. Control: unplanted solo campaign does **not** fail PersistBeforeSend; production path keeps the hook **off**.

**Why a unit test of “handle RequestVote” alone misses it:** the bug needs crash + recover + partition heal + timer injection across nodes so double-leader-in-term is *observable*. Persist-before-send engineering check can fail earlier on the planted hook alone (`planted_skip_vote_persist_fails_persist_before_send`).

---

## 12. Checker reminder (after events / end of run)

**White-box (`check.rs`):** Election Safety, Leader Append-Only, Log Matching, Leader Completeness, State Machine Safety; plus PersistBeforeSend/Ack, commit monotonicity, current-term commit, self matchIndex waits for Fsync; optional liveness under a separate flag.

**Black-box (`history.rs`):** linearizability on client history.

- Definite miss: `NotLeader`, `Invalid`.
- Unknown: `Io` (and incomplete/timeout-after-possible-apply) → search **both** apply and skip.
- Capacity overflow is not a Linearizability verdict.

---

## 13. Mental model to draw

```text
          seed
           │
           ▼
     swarm_plan / Rng
           │
           ▼
    ┌──────────────┐     Event      ┌─────────────────┐
    │   Cluster    │ ──────────────► │ chronos-protocol│
    │ scheduler    │ ◄────────────── │   Node::step    │
    │ net + disk   │     Effects     └─────────────────┘
    └──────┬───────┘
           │
           ▼
    checkers + history + SHA-256 trace
```

---

## 14. Drill: “Open the file that …”

| Question | Answer |
|----------|--------|
| …defines `step` | `chronos-protocol/src/raft/step.rs` |
| …persists votes / RequestVote after Fsync | `raft/election.rs` |
| …commits, applies, `replicate_all` | `raft/replication.rs` |
| …WAL CRC layout | `wal/record.rs` |
| …sim disk `bytes` / `durable_len` | `chronos-sim/src/disk.rs` |
| …ReplayBook / drain_horizon | `chronos-sim/src/cluster.rs` |
| …hashes traces | `chronos-sim/src/trace.rs` |
| …linearizability / Io unknown | `chronos-sim/src/history.rs` |
| …swarm seed runner | `chronos-sim/src/fuzz.rs` |
| …minify Recorded vs Defaults | `chronos-sim/src/minify.rs` |
| …plants skip vote persist | `tests/properties.rs` + `Node`/`Cluster` hook |
| …FileDisk drive loop | `chronos-node/src/main.rs` |
| …TCP stub | `chronos-node/src/net.rs` |

---

## 15. Read next

- Design locks & whiteboard → [F3](F3-architecture-and-design.md)
- Rust for this repo → [F4](F4-rust-for-chronos.md)
- War stories (281, drain, planted) → [F5](F5-problems-and-solutions.md)
- Drill bank → [F6](F6-interview-question-bank.md)
