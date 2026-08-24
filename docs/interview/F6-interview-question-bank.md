# F6 — Interview question bank (drill any scenario)

**Goal:** After this file you can drill fundamentals, Chronos specifics, system design, live coding, behavioral grilling, trap questions, and a scored 30-minute mock — out loud.

**How to use:** Cover the ask. Speak a strong answer. Compare. Practice the follow-up. Mark misses and retry next day.

**Markers:**

- **[CHRONOS]** — answer must match this repo / `docs/02-architecture.md`
- **[GENERAL]** — classic DS / Raft / interviewing

**Hard pins (fail the mock if you violate these):**

| Claim | Truth |
|-------|--------|
| Seed **281** | **CHECKER** (after harness recording fix) — **not** PROTOCOL |
| PLANTED pack | **Harness proof** — intentional `skip_vote_persist`; labeled PLANTED |
| Multi-node TCP HA | **Out of v1** — single-node WAL demo only |
| Wild Raft safety bug from swarm | **Do not claim** |
| HashMap + fixed hasher in sim | **Rejected** (D6) |

Also study: F1–F5, `docs/00-project-guide.md` §8, `docs/02-architecture.md` D1–D17.

---

# A. Fundamentals (DST, nondeterminism, Raft safety)

### A1. Why are distributed systems hard? **[GENERAL]**

**Ask:** In one minute, why is building distributed systems harder than a single process?

**Strong answer:** Partial failure (one node dies, others continue), no shared memory, no reliable global clock, networks delay/drop/duplicate/reorder, and failure is indistinguishable from slow. Correctness must be defined under schedules you don’t control in production.

**Weak answer:** “They’re just more servers” / only mentions “latency” / “CAP means pick 2 of 3” as a slogan without partition context.

**Follow-up:** Give an example where “the network lied” vs “the process crashed” look identical to a peer.

---

### A2. What is nondeterminism in a cluster test? **[GENERAL + CHRONOS]**

**Ask:** What makes the same test flake across runs?

**Strong answer:** Thread interleavings, wall clocks (`Instant::now`), OS RNG (including default `HashMap` seeding), real NIC timing, disk scheduling, and uncontrolled message delay. Chronos **architects away** these by one OS thread, virtual time, seeded PRNG, BTree collections, and simulated net/disk.

**Weak answer:** “Flaky asserts” only / blames CI without naming entropy sources.

**Follow-up:** Which of those still exist *outside* Chronos’s model? (Byzantine, true clock drift multi-DC, CPU cache — out of scope.)

---

### A3. What is deterministic simulation testing (DST)? **[GENERAL + CHRONOS]**

**Ask:** Explain DST and what artifact you get.

**Strong answer:** Run the system under a fully controlled logical scheduler: same seed ⇒ same event order ⇒ same trace digest. Inject faults from the PRNG. On failure: replayable seed, SHA-256 digest, minimized schedule, writeup. Chronos: `step` under sim and under node interpreter.

**Weak answer:** Equates DST with “more unit tests” or “Jepsen.”

**Follow-up:** How is DST complementary to Jepsen? (DST: huge schedule space, white-box; Jepsen: real ops chaos — Chronos v1 does not claim Jepsen.)

---

### A4. List Raft safety properties you check. **[GENERAL + CHRONOS]**

**Ask:** Name the safety properties and what they mean.

**Strong answer:**

1. **Election Safety** — at most one leader per term  
2. **Leader Append-Only** — leader never overwrite/delete own log entries  
3. **Log Matching** — same index+term ⇒ identical logs through that index  
4. **Leader Completeness** — committed entries appear in future leaders’ logs  
5. **State Machine Safety** — same index applied ⇒ same command  

Chronos also checks **persist-before-send/ack** white-box and **linearizability** black-box. Current-term commit / no-op on leadership relates to Figure 8.

**Weak answer:** Only says “consensus” / confuses liveness (eventual election) with safety.

**Follow-up:** Which property does skipping `votedFor` persist threaten? (Election Safety.)

---

### A5. Why randomized election timeouts? **[GENERAL]**

**Ask:** Why not fixed timeouts?

**Strong answer:** Split votes: two candidates, neither majority. Randomization desynchronizes retries so one likely wins. Chronos: harness owns durations; protocol emits `ArmTimer { kind: Election }` without picking absolute time (D15).

**Weak answer:** “To be faster” / puts `rand` inside protocol `step`.

**Follow-up:** Who may call RNG in Chronos — protocol or harness? (Harness only.)

---

### A6. Quorum intersection in one sentence. **[GENERAL]**

**Ask:** Why does majority quorum help?

**Strong answer:** Any two majorities share at least one node, so a new leader’s majority overlaps a previous commit majority — carrying committed entries forward (with Raft’s log rules).

**Weak answer:** “More than half is safer” without overlap.

**Follow-up:** What happens if you allow two leaders in one term? (Breaks the overlap story → Election Safety.)

---

### A7. Linearizability vs sequential consistency. **[GENERAL]**

**Ask:** Contrast them for a KV.

**Strong answer:** Linearizability: operations appear instantaneous at a single point between invoke and complete, respecting real-time order. Sequential: total order exists respecting per-client order, but real-time across clients can be weaker. Chronos black-box checker targets linearizability of the KV history.

**Weak answer:** Uses “strong consistency” as a synonym without definitions.

**Follow-up:** Why is Get-via-leader-memory dangerous without read-index? (Stale read → lin fail — why Chronos D7 Get-via-log.)

---

### A8. Replication ≠ durability. **[GENERAL]**

**Ask:** Why isn’t “replicated to 3 nodes” enough?

**Strong answer:** Correlated failure, missing fsync, same-disk firmware bugs, acknowledging before durable. Chronos models Append vs Fsync and optional fsync-lie (default **off**, D10).

**Weak answer:** “RAID handles it.”

**Follow-up:** Torn write vs fsync-lie in Chronos terms?

---

### A9. What does consensus provide? **[GENERAL]**

**Ask:** One crisp sentence.

**Strong answer:** Agreement on a growing log (or value) among correct nodes despite crashes and asynchronous delays — under the model’s failure assumptions (not Byzantine in Chronos v1).

**Weak answer:** “Makes the system available” (that’s liveness / product).

**Follow-up:** Chronos non-goals: Byzantine, membership, snapshots — say them.

---

### A10. Network partition vs crash. **[GENERAL]**

**Ask:** How does a peer tell them apart?

**Strong answer:** Often cannot. Timeouts fire either way. Raft uses terms and majorities so a minority partition cannot commit; majority elects a new leader; old leader steps down on higher term.

**Weak answer:** “Ping tells you.”

**Follow-up:** Chronos fault model: partitions are harness-scheduled, not OS iptables.

---

# B. Chronos-specific

### B1. What is Chronos in 60 seconds? **[CHRONOS]**

**Ask:** Elevator pitch.

**Strong answer:** Simulation-first Raft KV. Product is a deterministic correctness engine: same `step(node, event) → effects` under a seeded single-thread sim and a real node interpreter. Fuzz fault schedules; check Raft safety + linearizability; artifacts = seed, SHA-256 digest, minify, writeup. v1 ships PLANTED persist-before-vote harness proof; seed 281 closed as CHECKER. Non-goals: membership, snapshots, Jepsen, multi-node TCP HA.

**Weak answer:** “I implemented Raft in Rust” / claims PROTOCOL wild bug / claims multi-node cluster.

**Follow-up:** Name the three crates.

---

### B2. Explain `step`. **[CHRONOS]**

**Ask:** What are the inputs/outputs and constraints?

**Strong answer:** `step(&mut Node, Event) -> Vec<Effect>`. Synchronous. No `now`, no RNG, no fs/net. Events: timers, messages, IoComplete, Recover, ClientRequest. Effects: Send, IoSubmit, Arm/CancelTimer, Reply. Apply runs **inside** step (no Apply effect). Harness interprets effects.

**Weak answer:** “Async Raft tasks” / “step calls disk.write.”

**Follow-up:** Why completions instead of sync Disk::write? (D3 — crash between submit and complete.)

---

### B3. Persist-before-send / persist-before-ack. **[CHRONOS]**

**Ask:** What must be durable before which messages?

**Strong answer:** Before sending/granting **votes** (Meta term/votedFor), and before **AE-success** acks — durable cookie must cover what you claim. Not a blanket ban on Send with Append (leaders may replicate). Planted `skip_vote_persist` proves Election Safety oracle. White-box checker compares outgoing votes/acks against `durable`, not volatile hope.

**Weak answer:** “Fsync everything always before any Send” / “we don’t fsync votes.”

**Follow-up:** Which sofa-jraft class bug does this target?

---

### B4. `drain` vs `drain_horizon`. **[CHRONOS]**

**Ask:** When do you use each?

**Strong answer:**

| API | Stop condition | Use |
|-----|----------------|-----|
| `Cluster::drain()` | Live leader **and** heap has no **non-timer** events | Most unit/scenario tests |
| `Cluster::drain_horizon()` | Empty / past `max_ns` / check fail | Swarm, minify |

Idle heartbeats would burn `max_ns` if tests used horizon drain → CI hangs. Swarm needs horizon so late faults still fire.

**Weak answer:** Treats them as synonyms / puts horizon on idle tests.

**Follow-up:** Why did commit-advance need `replicate_all` even with drain? (Idle drain can stop before heartbeat carrying leaderCommit.)

---

### B5. Seed 281 — tell the story in <90s. **[CHRONOS]**

**Ask:** What failed, what you found, what you claim.

**Strong answer:** 1/1000 swarm fail, seed 281, Linearizability, reproducible digest. Minify first said MISMATCH. Triage HARNESS→CHECKER→SIM→PROTOCOL. **Harness:** ReplayBook missing heartbeat delay recording → live vs Recorded diverged. Fixed recording. **Checker:** Put completed `Err(Io)` then Get saw value; oracle treated Io as definite miss; architecture says Io is **unknown**. Fixed oracle. Seed 281 CLEAN. Classification **CHECKER**. No PROTOCOL pack. PLANTED remains G9 proof.

**Weak answer:** “We found a Raft bug” / “disabled linearizability” / “tuned drop rates.”

**Follow-up:** Why is a checker bug still a DST win?

---

### B6. PLANTED vs PROTOCOL vs CHECKER vs HARNESS. **[CHRONOS]**

**Ask:** Define the classes.

**Strong answer:**

- **HARNESS** — recording/replay/schedule plumbing wrong  
- **CHECKER** — oracle false fail/pass  
- **SIM** — world model bug (disk/net/time)  
- **PROTOCOL** — `step` violates invariant under legal schedule  
- **PLANTED** — intentional fault hook, labeled, swarm never enables  

Shipped: PLANTED ElectionSafety. 281: HARNESS then CHECKER. No wild PROTOCOL claim.

**Weak answer:** Calls PLANTED a production Raft bug find.

**Follow-up:** May minify enable `skip_vote_persist`? (No.)

---

### B7. Why ban HashMap in sim too? **[CHRONOS]**

**Ask:** D6 in your words.

**Strong answer:** Digests hash structured traces; checkers iterate maps. Default HashMap iteration order is nondeterministic across runs/platforms. Deterministic hasher is easy to forget and doesn’t fix all iteration sites. BTreeMap/BTreeSet/Vec in protocol **and** sim.

**Weak answer:** “HashMap is O(1)” / “fixed hasher is fine.”

**Follow-up:** Where may HashMap exist? (`chronos-node` OS tables that never iterate into traces.)

---

### B8. WAL format and torn writes. **[CHRONOS]**

**Ask:** On-disk framing?

**Strong answer:** Little-endian `[len][crc32(len||payload)][payload]`; Meta + Entry (+ Truncate) records. `durable_len` watermark; torn suffix is past durable. Scan stops at first bad/truncated header. No serde (D14). Recover passes durable prefix into `Event::Recover`.

**Weak answer:** JSON WAL / CRC only over payload / shrink durable_len to simulate tear (that’s fsync-lie territory).

**Follow-up:** Why CRC covers `len`?

---

### B9. Get path in Chronos. **[CHRONOS]**

**Ask:** How do reads work?

**Strong answer:** **Get via log** (D7) — client Get is a log command, not leader RAM. Avoids false lin and stale deposed-leader reads. Tradeoff: read latency like write path in v1.

**Weak answer:** “Leader returns memory for speed.”

**Follow-up:** What would you add later for faster linearizable reads? (Read-index — out of v1.)

---

### B10. Idempotency. **[CHRONOS]**

**Ask:** How do retries work?

**Strong answer:** Every client op carries `(ClientId, RequestId)`. Table is **state machine state** (D8), not a side cache. Same id → same response; Put applies once. At-least-once RPC safe.

**Weak answer:** Retry without keys / cache outside SM.

**Follow-up:** What breaks if replicas’ idempotency tables diverge?

---

### B11. IoId incarnation. **[CHRONOS]**

**Ask:** Why two fields?

**Strong answer:** `{incarnation, local}`. Local resets on Recover; incarnation never goes backward. Stale IoComplete ignored. Harness drops in-flight I/O on crash. Prevents post-restart aliasing.

**Weak answer:** “Just use a global counter forever” without crash story.

**Follow-up:** Who assigns MsgId vs IoId? (MsgId: harness at enqueue; IoId: node.)

---

### B12. Fsync-lie default. **[CHRONOS]**

**Ask:** Is firmware-lie injection on?

**Strong answer:** **Off by default (D10).** Always-on discovers known “paper Raft unsafe without real durability” and pollutes hunting. Can be a deliberate mode later; don’t claim default-on.

**Weak answer:** “We always lie to find more bugs.”

**Follow-up:** How does lie differ from torn write in the disk model?

---

### B13. Two checkers. **[CHRONOS]**

**Ask:** White-box vs black-box?

**Strong answer:** White-box: Raft safety + persist gates on observed roles/messages/cookies. Black-box: client invoke/complete history linearizability. KV can be wrong while Raft looks fine — need both (D12).

**Weak answer:** Only invariants on terms.

**Follow-up:** How should `ClientError::Io` be treated in lin search? (Unknown — seed 281 lesson.)

---

### B14. Minify success criterion. **[CHRONOS]**

**Ask:** What does delta-debug optimize for?

**Strong answer:** Minimal fault/delivery **atoms** that still fail the **same CheckName**, replaying with recorded delays/tokens. Digest may change. Success ≠ digest equality. Atoms are content tokens / fault windows — not raw MsgId ordinals that shift.

**Weak answer:** “Smallest digest” / removes timer events blindly until meaningless.

**Follow-up:** What did MINIFY MISMATCH mean on 281 before harness fix?

---

### B15. Heap key and time. **[CHRONOS]**

**Ask:** How is the event queue ordered?

**Strong answer:** Tickless `u64` ns. Heap key `(Timestamp, global seq)` (D4). No per-node seq bias. Protocol never sees wall clock.

**Weak answer:** Integer ticks + shuffle among ties as the main design / `step(node, now, event)` with clock in protocol.

**Follow-up:** Why ArmTimer kind instead of absolute deadline in effects? (D15.)

---

### B16. Cluster sizes and swarm counts. **[CHRONOS]**

**Ask:** What do you run in CI?

**Strong answer:** Swarm `n ∈ {3,5}`. PR ~32 seeds; nightly/dispatch ~1000; stretch 10k. Cite green 1000-seed GitHub run when asked (`32767856965` on commit pin in F1/handoff — verify before naming). Release tests, clippy, determinism gates.

**Weak answer:** Invents “millions of seeds exhaustive.”

**Follow-up:** Is exploration exhaustive? (No — coverage of what fired.)

---

# C. System design

### C1. Design a deterministic simulator for Redis-like KV. **[GENERAL]**

**Ask:** Whiteboard a DST harness for a single-key Redis subset (GET/SET) with replication optional.

**Strong answer:** Push: single-threaded event loop; virtual time; seeded RNG; separate pure state machine vs world; explicit net/disk effects or message queue; fault model (delay/drop/crash); history recorder; linearizability checker; trace hash; replay by seed. Compare to Chronos: Chronos adds Raft log, WAL durability, majority commit.

**Weak answer:** Threads + sleep + real Redis process as the “sim.”

**Follow-up:** What breaks if the Redis-like apply uses HashMap iteration in a digest?

---

### C2. Design a sim for Kafka-like log. **[GENERAL]**

**Ask:** Partitioned append log, producers/consumers, broker crash.

**Strong answer:** Model broker as step function; durable log watermark; in-flight produce acks gated on fsync; consumer offsets as state; inject broker crash + torn tail; checkers: no lost acknowledged produce, monotonic offsets, no gap in committed prefix. Single thread scheduler. Don’t claim full Kafka (ISR, exactly-once) in v1 sketch — scope explicitly.

**Weak answer:** Spawns real Kafka in Docker and calls it deterministic.

**Follow-up:** How do you encode “ack after durability” like Chronos persist-before-ack?

---

### C3. Design Raft disk durability. **[GENERAL + CHRONOS]**

**Ask:** Disk API + recovery.

**Strong answer:** Append-only checksummed records; separate Append vs Fsync; durable_len; crash truncates to durable; incarnation for in-flight I/O; recover scans valid prefix; Meta vs Entry. Chronos D9/D14/D17.

**Weak answer:** `fwrite` whole state each time / sync Disk trait inside protocol.

**Follow-up:** Why not in-place header update for term/votedFor?

---

### C4. Idempotent client API for at-least-once RPC. **[GENERAL + CHRONOS]**

**Ask:** Design keys and storage.

**Strong answer:** `(client_id, request_id)` required; store response in SM; leader applies once; retries return stored resp. Chronos D8.

**Weak answer:** UUID in client only, forgotten after restart of server SM.

**Follow-up:** Interaction with session lifetimes / client crash?

---

### C5. Fault injection for CI. **[CHRONOS-shaped]**

**Ask:** How do you schedule faults without flakes?

**Strong answer:** Seed → config profile (calm/brutal); PRNG draws delays/crashes/partitions/fsync fail; record schedule for replay; coverage counters of observed faults; fail artifacts (seed, digest, minify); PR N vs nightly N; never greenwash by disabling checkers.

**Weak answer:** Random `thread::sleep` in tests.

**Follow-up:** What belongs in a bug pack writeup?

---

### C6. Adding membership changes. **[CHRONOS]**

**Ask:** How would you extend Chronos?

**Strong answer:** New protocol phase — joint consensus carefully; don’t pretend v1 has it (D11). Separate checkers; more schedule space; snapshots maybe after logs hurt. Explicitly: not a small patch.

**Weak answer:** “Just add nodes to the peer Vec.”

**Follow-up:** Why is membership a second protocol?

---

### C7. Observability for a sim. **[GENERAL]**

**Ask:** What do you log/metrics without breaking determinism?

**Strong answer:** Structured TraceRecords from logical events only; host fields excluded from digest (D13); counters of fault types; check names; avoid timestamps from wall clock in the hashed stream.

**Weak answer:** `println!(Instant::now())` in protocol.

**Follow-up:** Why SHA-256 of encoded traces vs `std::hash::Hasher`?

---

# D. Live coding (G-list)

Practice on a whiteboard or editor. Aim 20–40 minutes each. Speak invariants while coding.

### D1 / G1. Encode a WAL record. **[CHRONOS]**

**Ask:** Implement `encode_record(payload: &[u8]) -> Vec<u8>` as `[len LE][crc(len||payload) LE][payload]`.

**Strong answer:** `len` as u32 LE; CRC over len bytes ‖ payload; append crc LE; append payload. Mention decode/scan stop on torn/bad CRC. No serde.

**Weak answer:** JSON / CRC payload-only / big-endian without stating platform story.

**Follow-up:** Write `scan` that returns `(records, valid_len)`.

---

### D2 / G2. Porcupine-style sketch for a register. **[GENERAL + CHRONOS]**

**Ask:** Single-key register; ops with invoke/complete times; is history linearizable? Discuss failed Put with `Io`.

**Strong answer:** Search interleavings respecting real-time and per-op intervals; state is last written value. **Io** = unknown (apply or not); **NotLeader** = definite miss. Seed 281: Io then Get value must be allowed.

**Weak answer:** Treats all errors as never applied.

**Follow-up:** Bound complexity — why Chronos keeps histories small per run.

---

### D3 / G3. Election timeout without `Send`. **[CHRONOS]**

**Ask:** On election timeout, candidate increments term, votes for self, submits Meta persist — show effects **without** sending RequestVote yet.

**Strong answer:** Effects: IoSubmit Append Meta, IoSubmit Fsync, ArmTimer; maybe pending vote messages gated on durable. Persist-before-send. Contrast planted hook that Sends early.

**Weak answer:** Immediately Send RequestVote in same batch as IoSubmit without gate.

**Follow-up:** What does the checker assert on outbound votes?

---

### D4 / G4. Delta-debug a schedule. **[CHRONOS]**

**Ask:** Pseudocode `minimize(atoms, fails) -> minimal subset`.

**Strong answer:** Classic DD: try removing halves; recurse on failing subset; then 1-by-1. Predicate = replay Recorded delays + subset atoms ⇒ same CheckName. Don’t use live RNG in the inner loop.

**Weak answer:** Randomly delete events until flaky.

**Follow-up:** What are Chronos atoms vs MsgId ordinals?

---

### D5 / G5. BTree vs Hash for traces. **[CHRONOS]**

**Ask:** Implement a tiny “observe leaders per term” structure; argue collection choice.

**Strong answer:** `BTreeMap<Term, NodeId>` — at most one leader; iteration stable for digest/debug. HashMap banned in sim.

**Weak answer:** HashMap + “we’ll sort sometimes.”

**Follow-up:** Show Election Safety check using the map.

---

### D6 / G6. Durable watermark. **[CHRONOS]**

**Ask:** Struct `{ bytes: Vec<u8>, durable_len }`; methods `append`, `fsync_ok`, `crash_torn`.

**Strong answer:** append grows bytes; fsync_ok advances durable_len to sync point; crash truncates bytes to durable_len (torn = lost undurable suffix). Invariant: durable_len ≤ bytes.len().

**Weak answer:** Shrinks durable_len without truncating (lie) accidentally.

**Follow-up:** Map to Chronos IoComplete Ok vs Err.

---

### D7 / G7. Idempotent apply. **[CHRONOS]**

**Ask:** Apply Put with client/request ids.

**Strong answer:** Lookup table; if hit return stored; else mutate KV, store resp, return. Table in SM state.

**Weak answer:** Apply every time; “client shouldn’t retry.”

**Follow-up:** Get with same request id behavior.

---

### D8 / G8. Schedule heap tie-break. **[CHRONOS]**

**Ask:** Push events same timestamp; pop order.

**Strong answer:** Key `(t, seq)` with monotonic global seq. Deterministic among ties (D4).

**Weak answer:** PRNG among equal times as primary design / per-node seq.

**Follow-up:** Why per-node seq biases by NodeId?

---

### D9 / G9. Persist cookie gate. **[CHRONOS]**

**Ask:** `can_send_vote(durable: Cookie, needed: Cookie) -> bool`.

**Strong answer:** Compare term/voted_for/(log fields as needed) — durable must dominate pending claim before Send.

**Weak answer:** Always true if in_flight empty.

**Follow-up:** AE-success path differences.

---

### D10 / G10. Stale timer / stale Io after cancel or crash. **[CHRONOS]**

**Ask:** Pseudo-harness: cancel timer then fire old deadline; or complete old IoId.

**Strong answer:** Ignore stale timer ids / incarnation mismatch; tests exist for cancel-then-rearm and crash-before-fsync.

**Weak answer:** Deliver anyway “to be safe.”

**Follow-up:** How Recover interacts with in-flight disk ops.

---

# E. Behavioral / resume grilling

### E1. Tell me about yourself → Chronos. **[CHRONOS]**

**Ask:** Bridge resume to project.

**Strong answer:** Systems/correctness interest → chose simulation-first consensus so interviewers can audit architecture. Owned effect-based step, three crates, sim, checkers, fuzz/minify, CI, bug packs. Speak “I” only for what you can defend from the repo.

**Weak answer:** Team-washed “we” for solo work / claims tools you didn’t run.

**Follow-up:** What would you delete from the resume bullet list as overclaim?

---

### E2. Biggest disagreement / hard decision. **[CHRONOS]**

**Ask:** Tradeoff you made.

**Strong answer:** Examples: D2 own scheduler vs MadSim; D7 Get-via-log vs fast reads; D10 fsync-lie off; D3 effects vs sync Disk; drain vs horizon split. State rejected alternative and how it bites later.

**Weak answer:** “No disagreements” / bikeshed formatting.

**Follow-up:** Which decision would you revisit at 10× scale?

---

### E3. Failure you owned. **[CHRONOS]**

**Ask:** Talk about a miss.

**Strong answer:** Seed 281 — owned triage discipline; fixed harness then checker; refused fake PROTOCOL narrative. Or CI hang from wrong drain mode. Show learning.

**Weak answer:** Blames CI / hides the checker bug.

**Follow-up:** What process change stuck? (Record all PRNG consumers; Io as unknown.)

---

### E4. Why should we hire you given Chronos? **[CHRONOS]**

**Ask:** Signal beyond “finished a project.”

**Strong answer:** Architects for testability; honest evidence classes; production-grade gates (clippy, determinism grep); debugging under uncertainty; scoped non-goals. Strongest on systems design; Rust learned in service of the design.

**Weak answer:** Buzzword Antithesis/Jepsen without running them.

**Follow-up:** What would you do in first 90 days on a real storage team?

---

### E5. Conflict between speed and correctness. **[CHRONOS]**

**Ask:** Example.

**Strong answer:** Get-via-log slower but prevents false confidence; fsync-lie off avoids junk finds; release tests slower but stop hangs.

**Weak answer:** Always picks speed.

**Follow-up:** When is a planted bug valid portfolio evidence?

---

### E6. Working with incomplete specs. **[CHRONOS]**

**Ask:** How did architecture docs help?

**Strong answer:** `02-architecture.md` wins conflicts; D-locks prevent thrash; roadmap phases; bug packs as institutional memory.

**Weak answer:** “I just coded.”

**Follow-up:** Show one D-lock and the rejected alternative.

---

# F. Trap questions (fail if you overclaim)

### F1. “So DST found a Raft bug?” **[CHRONOS]**

**Ask:** (Trap.)

**Strong answer:** PLANTED harness proves Election Safety detection. Seed 281 was CHECKER (+ harness recording). We did **not** ship a wild PROTOCOL Raft safety pack from swarm.

**Weak answer:** “Yes, swarm found split brain in our Raft.”

**Follow-up:** What would a real PROTOCOL pack contain?

---

### F2. “Is this like Jepsen?” **[CHRONOS]**

**Ask:** (Trap.)

**Strong answer:** Complementary ideas; Chronos is deterministic simulation with white-box + lin checkers. Jepsen-style real-cluster chaos is **out of v1**. Don’t equate.

**Weak answer:** “Yes, Chronos is Jepsen.”

**Follow-up:** When would you add Jepsen beside DST?

---

### F3. “Get from leader memory is fine, right?” **[CHRONOS]**

**Ask:** (Trap.)

**Strong answer:** Breaks linearizability without read-index/lease; deposed leader serves stale. Chronos D7 Get-via-log.

**Weak answer:** Agrees for performance.

**Follow-up:** Sketch read-index briefly as future work.

---

### F4. “HashMap with a fixed hasher is OK in the sim.” **[CHRONOS]**

**Ask:** (Trap.)

**Strong answer:** Rejected by D6. Ban HashMap in protocol **and** sim; use BTree*. Iteration sites are easy to miss.

**Weak answer:** Agrees with fixed hasher.

**Follow-up:** Name two sim iteration sites that enter digests.

---

### F5. “Turn on fsync-lie by default to find more bugs.” **[CHRONOS]**

**Ask:** (Trap.)

**Strong answer:** D10 off by default — pollutes with known paper-Raft failures. Intentional mode ≠ default.

**Weak answer:** “Always on is more hardcore.”

**Follow-up:** How do you still test durability bugs? (Torn, fsync Err, persist-before-*, planted.)

---

### F6. “You run a multi-node TCP cluster in prod mode.” **[CHRONOS]**

**Ask:** (Trap.)

**Strong answer:** v1 `chronos-node` is a **single-node** WAL interpreter demo. Multi-node networking is non-goal for v1. Logical multi-node lives in the **sim**.

**Weak answer:** Claims HA TCP cluster on resume.

**Follow-up:** What would multi-node node mode need beyond protocol?

---

### F7. “Why not Tokio + MadSim under Raft — that’s modern?” **[CHRONOS]**

**Ask:** (Trap.)

**Strong answer:** D2 rejected — hides step, scheduler entropy, Windows/tooling pain, HashMap leaks remain. Own single-thread heap.

**Weak answer:** Apologizes and says you’d rewrite on Tokio.

**Follow-up:** When *would* async be appropriate in the wider product?

---

### F8. “Minify must preserve digest.” **[CHRONOS]**

**Ask:** (Trap.)

**Strong answer:** Success = same **CheckName**, not digest equality. Schedule subset changes the trace; fault still triggers same check.

**Weak answer:** Requires identical digest.

**Follow-up:** 281 MINIFY MISMATCH meaning?

---

### F9. “Coverage means all schedules explored.” **[CHRONOS]**

**Ask:** (Trap.)

**Strong answer:** No. Coverage counters what fault/delay classes fired. Space is enormous; seeds sample.

**Weak answer:** Claims exhaustive.

**Follow-up:** How do you raise confidence anyway? (More seeds, profiles, minify, planted proofs.)

---

### F10. “Io error means the Put never applied.” **[CHRONOS]**

**Ask:** (Trap.)

**Strong answer:** False. Fsync can fail after entry may still commit via other paths / races in scheduling; treat Io as **unknown** in lin (281).

**Weak answer:** Definite miss like NotLeader.

**Follow-up:** Which errors are definite?

---

# G. Mock 30-min script with scoring

## G.0 Setup

- Interviewer: uses this script; interrupt with follow-ups from A–F.  
- Candidate: no notes except blank whiteboard.  
- Record audio if practicing alone.

## G.1 Agenda (30:00)

| Time | Block | What |
|------|-------|------|
| 0:00–5:00 | Pitch | F1 one-minute + non-goals + honest Rust line if asked |
| 5:00–13:00 | Architecture | Draw crates, step/effects, disk Append/Fsync, heap key |
| 13:00–20:00 | War story | Seed 281 **or** PLANTED — classify correctly |
| 20:00–25:00 | Raft deep dive | Persist-before-send + Election Safety |
| 25:00–30:00 | Live coding | Pick **one**: D1 encode WAL, D5 BTree leaders, D8 heap key |

## G.2 Scoring rubric (100 points)

| Area | Points | Award full only if… |
|------|--------|----------------------|
| Pitch + non-goals | 15 | Mentions sim-first, checkers, PLANTED; **no** multi-node TCP / Jepsen / wild PROTOCOL |
| Architecture accuracy | 20 | step/effects, D2/D6, durable_len, three crates |
| 281 / PLANTED honesty | 20 | 281 = CHECKER; PLANTED = harness proof |
| Raft persist story | 15 | Votes/AE-success gated; not blanket Send ban |
| Live coding | 15 | Compiles mentally; cites Chronos invariant |
| Communication | 10 | Crisp; admits unknowns; uses “I’d verify in repo” |
| Trap resistance | 5 | Survives at least one F-trap without folding |

**Pass bar:** ≥ 75 and **zero** critical fails.

**Critical fails (automatic mock fail):**

1. Calls seed 281 a PROTOCOL Raft bug  
2. Claims multi-node TCP HA in v1  
3. Says HashMap + fixed hasher is fine in sim  
4. Equates Chronos with Jepsen as shipped  

## G.3 Interviewer prompts (use liberally)

- “Where does RNG live?”  
- “Show me drain vs horizon.”  
- “Is apply an effect?”  
- “What’s in the PLANTED pack path?”  
- “Why little-endian?”  
- “Explain IoId.”  
- “You’re wrong about 281 — defend.” (candidate must still hold CHECKER with evidence)

## G.4 Candidate recovery line (if stuck)

> I don’t want to invent an answer. In Chronos the source of truth is `docs/02-architecture.md` decision Dx / the code in `crates/…`. Here’s what I remember and how I’d verify…

## G.5 After-action

Write 5 bullets: misses, traps hit, one sentence to memorize, next drill list (e.g. B5, D2, F1). Retake mock within 48 hours if critical fail.

---

# Quick reference — count and coverage

This bank includes **40+** fully structured questions (Ask / Strong / Weak / Follow-up) across:

| Section | Focus | Approx. count |
|---------|--------|----------------|
| A | DST / DS / Raft fundamentals | 10 |
| B | Chronos architecture & bugs | 16 |
| C | System design prompts | 7 |
| D | Live coding G-list | 10 |
| E | Behavioral / resume | 6 |
| F | Traps | 10 |
| G | Mock + scoring | script |

**Night-before subset (45 min):** B1, B2, B3, B4, B5, B6, B7, D1, D4, F1, F4, F6, G mock once.

**Do not claim:** wild PROTOCOL from swarm; multi-node TCP HA; Jepsen/Antithesis shipped; fsync-lie default-on; HashMap-OK-in-sim; seed 281 as Raft bug.

**Do claim:** simulation-first Raft KV; deterministic swarm; PLANTED harness proof; 281 CHECKER close; CI gates; honest non-goals.
