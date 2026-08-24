# F1 — Project pitch and spoken interview script

**Audience:** ~30 LPA+ backend interviews. Chronos is your core resume project.  
**Assumption:** You may not know Rust yet. This file is for speaking aloud, not for reading silently once.  
**Source of truth:** This briefing + common Raft knowledge (labeled **Raft paper**). Spec `docs/02-architecture.md` wins conflicts. Repo: https://github.com/sahilahmed21/Chronos  

**Do not invent PROTOCOL bugs.** Prefer wild PROTOCOL *not found*. Seed 281 = CHECKER. G9 artifact = PLANTED pack.

---

## 0. How to use this file (study drill)

1. Day 1: memorize **§2 sixty-second** and **§3 five-minute** until you can say them without looking.  
2. Day 2: record yourself doing **§5 full spoken script** (aim 8–12 minutes with pauses).  
3. Day 3: practice **§8 red flags** — if you catch yourself claiming multi-node HA or a wild PROTOCOL find, restart.  
4. Day 4: whiteboard from **§7** on paper with a timer (10 minutes).  
5. Before interview: skim **§6 resume bullets**, **§9 why Rust**, **§10 what next**, **§11 day-before checklist**.

---

## 1. What Chronos is (plain English — own this)

**Chronos** is a **simulation-first Raft key-value store**. The product is not “another Raft clone on GitHub.” The product is a **correctness engine**.

Three ideas you must say out loud:

1. **Same protocol, two worlds.** The Raft + KV logic runs under:
   - **`chronos-sim`** — *we* own time, network, disk, and RNG (deterministic simulation).
   - **`chronos-node`** — the OS owns files (v1: **single-node WAL demo only**, not multi-node TCP HA).
2. **Core API:**

   ```text
   fn step(node, event) -> Vec<Effect>
   ```

   Protocol code takes a node state + an event and returns a list of **effects** (send message, submit disk I/O, arm timer, reply to client). It does not own the OS. Interpreters apply effects.
3. **Failure artifact:** when a property fails, you get a **seed** + **SHA-256 digest** + (when minify works) a **minimized schedule** — something you can replay, not a flake.

**Three crates:**

| Crate | Job |
|-------|-----|
| `chronos-protocol` | Raft + KV `step` / effects (no OS I/O in the core) |
| `chronos-sim` | Deterministic world: swarm / replay / minify / coverage + checkers |
| `chronos-node` | Real-file interpreter path; **v1 = single-node WAL demo** |

**Spec:** `docs/02-architecture.md` wins all design conflicts.

**Name intuition:** Chronos ≈ we control *time* in the sim (tickless virtual time), so schedules are reproducible.

---

## 2. Sixty-second pitch (memorize word-for-word, then personalize)

Speak this in ~55–70 seconds. Do not rush.

> Chronos is a simulation-first Raft key-value store. The product is a **correctness engine**, not just “I implemented Raft.” The same protocol code — a `step(node, event)` function that returns effects — runs under a deterministic simulator where we own time, network, disk, and randomness, and under a real node interpreter where the OS owns files. We fuzz thousands of fault schedules from a seed, check Raft safety properties and linearizability, and on failure we keep a seed, a SHA-256 digest, and a minimized schedule you can replay. v1 ships election and replication on a static 3 or 5 node cluster, Get and Put through the log with idempotency, and CI including a planted ElectionSafety harness proof. We also closed a swarm Linearizability hit on seed 281 as a **checker false fail**, not a Raft protocol bug. Explicitly out of scope for v1: multi-node TCP HA, snapshots, membership, Jepsen, and Byzantine faults.

**Drill tip:** If you go over 90 seconds, you are adding scope claims. Cut.

---

## 3. Five-minute pitch (structure + spoken paragraphs)

Use this when they say “Tell me more” or “Walk me through the project.” Aim ~4.5–5.5 minutes. Pause after each block for questions.

### Block A — Problem (45–60s)

> Distributed systems bugs are often **schedule-dependent**. Message A slightly before B, an fsync stall during election, a crash between append and durable Meta — those paths are rare and hard to reproduce. Unit tests that only assert “function returns Y given X” do not explore interleavings. Flakes that only fail on one OS or one timing are useless for debugging.
>
> Chronos’s bet: **architect away nondeterminism**. Put a whole logical cluster in a deterministic simulator, drive delays, drops, crashes, and partitions from a **seeded RNG**, and check safety after events. Same seed should give the same SHA-256 digest.

### Block B — Approach (60–75s)

> The design is three crates. `chronos-protocol` is the pure-ish core: `step` returns effects — it does not call the OS, does not read a wall clock, does not roll dice. `chronos-sim` owns the world: time, network, disk, RNG, swarm fuzz, replay, minify, coverage. `chronos-node` is the production-style interpreter path with real files — in v1 that is a **single-node WAL demo**, not a multi-process HA cluster.
>
> That split matters for interviews: I can talk about **protocol correctness** separately from **I/O and scheduling**. Spec is `docs/02-architecture.md`.

### Block C — Raft / KV choices (60s)

> For Raft itself — **Raft paper** — we implement leader election and log replication on a **static** 3 or 5 node membership. No dynamic membership, no snapshots in v1. Client Get and Put go through the **log**, not “leader memory is truth.” We care about **idempotency** for client retries. We enforce **persist-before-send** for votes and **persist-before-ack** for AppendEntries success paths — you do not advertise durable state you have not persisted. Checkers cover **five Raft safety properties** plus those persist rules plus **linearizability** on client history.

### Block D — Product loop (45–60s)

> The loop is: pick a seed → run swarm under fault profiles → run checkers → if something fails, keep seed + digest → replay → minify the fault schedule. The artifact is something another engineer can re-run. That is the product.

### Block E — Evidence + honesty (45–60s)

> Evidence I can point to: CI green; a **PLANTED** pack under `docs/bugs/2026-08-24-planted-skip-vote-persist/` that proves ElectionSafety when we intentionally skip vote Meta persist — test-only hook, swarm never enables it. Separately, seed **281** failed Linearizability in a 1000-seed run; triage closed it as **CLASS=CHECKER** — Io treated as definite miss when it should be unknown — plus a heartbeat recording gap that broke minify. After fixes, 1000-seed Docker and GitHub dispatch were green. We did **not** ship a wild PROTOCOL Raft safety bug pack. Prefer saying wild PROTOCOL was **not found**.

### Block F — Stop (15s)

> Happy to go deep on the step function, the planted ElectionSafety demo, or the seed-281 triage — whichever is most useful.

---

## 4. v1 IN vs OUT (say the OUT list unprompted once)

### IN (shipped — claim these)

| Piece | Claim |
|-------|--------|
| Raft | Election + replication |
| Membership | Static **3 or 5** nodes |
| KV | Get / Put via **log**; idempotency |
| Sim | Swarm / replay / minify / coverage |
| Checkers | **5 Raft safety** + persist-before-send/ack + linearizability |
| G9 | PLANTED pack ElectionSafety (`docs/bugs/2026-08-24-planted-skip-vote-persist/`) |
| CI | Green; 1000-seed Docker + GitHub dispatch green after 281 close |
| Artifact model | Seed + SHA-256 digest + minimized schedule |

### OUT (non-goals — say these so they cannot trap you)

- Multi-node **TCP HA** cluster  
- Snapshots / compaction  
- Dynamic **membership**  
- **Read-index** / lease reads  
- **Jepsen**, **Antithesis**  
- **Fsync-lie** default-on  
- **Byzantine** faults  
- **Tokio under the protocol**  
- `chronos-node` as anything more than **single-node WAL demo**

**Spoken line:**

> If you ask what I’d ship next for a real database product — membership, snapshots, multi-node networking — those are explicitly v1 OUT. Chronos v1 is a correctness lab with a single-node WAL demo on the node path.

---

## 5. Full spoken script (drill this end-to-end)

Use this as a continuous talk. Sections are labeled so you can jump if they interrupt. Speak naturally; do not sound like you are reading a README.

---

### 5.1 Opening — “Tell me about Chronos”

> Chronos is my core project: a **simulation-first Raft KV**. I did not build it primarily to say “I cloned Raft.” I built it to own a **correctness engine** — deterministic fault injection, property checkers, and a replayable artifact when something fails.
>
> The center of the design is one function shape: `step(node, event)` returns a vector of effects. The protocol does not own sockets or files. The simulator owns time, network, disk, and RNG. The node crate interprets the same protocol against real files for a single-node WAL demo.
>
> Repo is public: github.com/sahilahmed21/Chronos. Architecture decisions live in `docs/02-architecture.md`.

*[Pause. Let them steer.]*

---

### 5.2 Architecture — three crates + step

> There are three crates. `chronos-protocol` holds Raft election and replication plus the KV log semantics. `chronos-sim` is the deterministic world: swarm fuzz, replay, minify, coverage, and the checkers. `chronos-node` is the OS-backed interpreter — v1 is intentionally a **single-node WAL demo**, not multi-node TCP HA.
>
> Why `step` → effects? Because if Raft code calls `fsync` or `sleep` or `random` directly, you cannot replay schedules. Effects make I/O and time **external**. The sim applies them under a seed. That is how we get the same digest across runs.
>
> Client Get and Put go through the **log**. That matters for linearizability stories: truth is what committed and applied, not what sat in a leader’s memory.

*[If they ask Raft basics — use Raft paper language.]*

> From the **Raft paper**: election chooses a leader; the leader replicates a log; safety properties include at most one leader per term, and log matching / leader completeness style guarantees. We check **five Raft safety** properties in the sim, plus persist-before-send and persist-before-ack, plus linearizability on the client history.

---

### 5.3 Demo story — PLANTED ElectionSafety (G9)

> When interviewers ask “did simulation find a bug?”, I separate **harness proof** from **wild finds**.
>
> For G9 we shipped a **PLANTED** pack: `docs/bugs/2026-08-24-planted-skip-vote-persist/`. There is a **test-only** hook `skip_vote_persist`. Swarm **never** enables it. With the hook on, a node can send or grant votes before Meta is durable; after crash and recover it can vote again in the same term — and **Election Safety** fails. That is the classic “persist before you vote” failure mode. The pack proves the checker and the persist-before-send story work.
>
> I label it **PLANTED** on purpose. I do **not** claim it as a wild PROTOCOL discovery from swarm.

*[Optional one-liner if they push “isn’t that cheating?”]*

> No — it is a regression harness. Production and swarm keep the hook off. Preferring a labeled plant over a fake wild find is the honest engineering move.

---

### 5.4 War story — seed 281 (CHECKER + heartbeat book)

> The interesting wild path was seed **281** in a 1000-seed swarm. Linearizability failed. Replay could hit it; minify first did not — we got a **MINIFY MISMATCH**.
>
> Triage discipline was: harness and checker before blaming Raft. First issue: **heartbeat jitter** was not recorded in the **ReplayBook**, so live vs recorded worlds diverged. We fixed heartbeat delays into the book with FIFO recording, like election delays. That fixed the mismatch.
>
> Then the real classification: **CLASS=CHECKER**. A Put completed with `Err(Io)`, and a later Get returned `Ok(value)`. The oracle treated Io as a **definite miss**. Architecture says an Io after a possible apply is **unknown** — the put may have applied or not. Treating it as definite miss made a later successful Get look illegal. Fix: Io is searched as **apply or skip** (unknown). After that, seed 281 closed clean. 1000-seed Docker and GitHub dispatch were green.
>
> Bottom line: DST found a real bug in our **oracle and recording**, not a wild Raft PROTOCOL safety bug. We did not invent a PROTOCOL pack for the portfolio.

---

### 5.5 Scope boundaries — what I will not claim

> v1 does **not** include multi-node TCP HA, snapshots, membership changes, read-index, Jepsen, Antithesis, fsync-lie default-on, Byzantine faults, or Tokio under the protocol. `chronos-node` is a single-node WAL demo. If you need production HA, that is future work on top of the correctness lab — not something I will claim Chronos already is.

---

### 5.6 Closing

> If we have time, I can whiteboard `step` and effects, walk Put through the log, or redo the seed-281 triage order. I can also talk about smaller CI and sim fixes — drain versus drain_horizon, commit advancing with replicate_all, election timeouts versus RTT — those show the day-to-day debugging, not just the headline story.

---

## 6. Resume bullets (copy-paste accurate)

Use these or near-paraphrases. Do not upgrade “sim” into “production HA cluster.”

- Built **Chronos**, a simulation-first Raft KV in Rust: shared `step(node, event) → effects` protocol under a deterministic sim (owned time/net/disk/RNG) and a single-node WAL node interpreter.
- Implemented Raft **election + replication** on static 3/5 node clusters; Get/Put via log with idempotency; checkers for five Raft safety properties, persist-before-send/ack, and linearizability.
- Built swarm fuzz, SHA-256 digests, replay, and schedule minify; CI green including 1000-seed Docker / GitHub dispatch after closing seed 281.
- Shipped a **PLANTED** ElectionSafety pack (`skip_vote_persist` test-only; swarm never enables); triaged seed 281 Linearizability to **CHECKER** (Io as unknown) + heartbeat ReplayBook fix — no wild PROTOCOL claim.

**Shorter three-bullet variant (tight resume):**

- Simulation-first Raft KV: pure `step` protocol + deterministic sim + single-node WAL demo.
- Property-based swarm with seed/digest/minify; Raft safety + linearizability checkers.
- PLANTED ElectionSafety harness; seed 281 closed as checker false fail; CI 1000-seed green.

---

## 7. Whiteboard outline (10-minute draw)

Draw this left-to-right. Practice until you can fill a board without notes.

```text
[Client] --ClientRequest--> [chronos-protocol::step]
                                |
                                v
                         Vec<Effect>
                    (Send / Disk / Timer / Reply)
                                |
              +-----------------+------------------+
              |                                    |
              v                                    v
        chronos-sim                          chronos-node
     (we own time/net/disk/RNG)            (OS owns files)
     swarm / replay / minify               v1: single-node WAL demo
     checkers: 5 Raft safety
               + persist-before-send/ack
               + linearizability
              |
              v
     fail artifact: seed + SHA-256 digest + minimized schedule
```

**Talk track while drawing:**

1. Client request becomes an event into `step`.  
2. `step` never fsyncs itself — it returns disk effects.  
3. Sim interprets effects under a seed; node interprets against a real WAL.  
4. Checkers run on sim world + client history.  
5. Failures produce seed + digest + minify schedule.  
6. Box in corner: **v1 OUT** — no multi-node TCP HA, no snapshots, no membership.

**Optional second board (Raft paper only):**

```text
Follower --timeout--> Candidate --RequestVote--> majority --> Leader
Leader --AppendEntries--> Followers; commit when majority durable; apply; Reply
```

Say: “This is standard **Raft paper** election + replication; Chronos’s twist is deterministic sim + checkers + artifacts.”

---

## 8. Red flags / lies that fail interviews

If you say any of these, expect a follow-up that collapses your story.

| Lie / overclaim | Why you die | Honest replacement |
|-----------------|-------------|--------------------|
| “Swarm found a Raft safety PROTOCOL bug” | Seed 281 was CHECKER; G9 is PLANTED | “PLANTED ElectionSafety proof; wild PROTOCOL not found; 281 was checker” |
| “Chronos is a multi-node production HA KV” | Node is single-node WAL demo; TCP HA is OUT | “Correctness engine + single-node WAL demo” |
| “We run Jepsen / Antithesis” | Explicitly OUT | “In-sim checkers + swarm; Jepsen is future” |
| “Fsync-lie is on by default” | OUT / not default-on | “Modeled or future; not the v1 default story” |
| “We support snapshots / membership / read-index” | OUT | “Static 3/5; no snapshots/membership/read-index in v1” |
| “Tokio drives the protocol” | Tokio under protocol is OUT | “Own sim loop; protocol is step/effects” |
| “I’m fluent in Rust / production Rust expert” | You may still be learning | See §9 |
| “Linearizability failed because Raft applied wrong” for 281 | False — Io definite-miss oracle | “Oracle treated Io as definite miss; fixed as unknown” |
| “Minify mismatch means Raft nondeterminism” for 281 | Heartbeat not in ReplayBook | “Recording gap on heartbeat jitter” |
| “Planted hook runs in swarm” | Swarm never enables it | “Test-only; swarm never enables skip_vote_persist” |

**Emergency recovery line if you overclaim:**

> Let me correct myself — that is v1 OUT / not what we shipped. What we actually have is …

Interviewers respect the correction more than the original boast.

---

## 9. How to answer “Why Rust?” without claiming fluency

You are preparing while **learning** Rust. Do not fake senior Rust. Tie the answer to Chronos’s needs.

### Spoken answer (45–60s)

> I chose Rust for Chronos because the project cares about **determinism, explicit effects, and memory safety without a GC pause story** in a single-threaded sim. Enums for events and effects, Result for Io/NotLeader style errors, and strong typing around log indexes and terms fit a Raft state machine. I’m **still learning** idiomatic Rust — ownership, lifetimes, clippy gates — and I treat the Chronos codebase as the way I learn, not as proof I’m a Rust language lawyer. For this interview I’m happy to discuss the **systems design** of Chronos in depth; for Rust syntax I’ll be honest about what I know versus what I’d look up.

### Good follow-ups you can handle without “fluency”

- Why not put Tokio under the protocol? → Determinism / owned step loop; Tokio-under-protocol is v1 OUT.  
- Why care about SHA-256 digests? → Cross-run / cross-machine equality of traces.  
- What is an Effect? → Something the interpreter must do: send, disk, timer, reply.

### Bad answers

- “Rust is the best language” with no Chronos link.  
- “I’m an expert in async Rust / Tokio internals” while Chronos deliberately avoids Tokio under protocol.  
- Pretending you wrote unsafe exotic Rust you cannot explain.

---

## 10. “What would you do next?” answers

Pick 2–3. Match the interviewer’s company (infra vs product).

### If they want product / HA database next

> First: multi-node networking so `chronos-node` is a real cluster, not only a single-node WAL demo. Then snapshots and log compaction, then membership changes — all currently v1 OUT. I’d keep the same `step` protocol and grow interpreters + ops.

### If they want correctness / DST next

> Turn on harder disk fault modes carefully — fsync-lie style behavior is explicitly not the v1 default story. Add stronger external validation later (Jepsen-style) once multi-node exists. Keep triage discipline: HARNESS → CHECKER → SIM → PROTOCOL so we never fake PROTOCOL packs.

### If they want Raft completeness next

> Read-index / lease reads for cheaper linearizable reads — OUT today because Get goes through the log. Membership for operator reality. Snapshots for log growth. Still: no Byzantine claims.

### If they ask “would you rewrite in Go/Java?”

> The valuable part is the **architecture**: pure step, owned sim, checkers, seed artifacts. Language is secondary. Rust helped with enums/Result and “no hidden GC pauses in the sim thread,” but I’d port the *design* before I’d worship the language.

### One crisp default closer

> Next milestone I’d prioritize: multi-node node path + snapshots, while keeping simulation-first checkers as the merge gate — because the product of Chronos is still the correctness engine.

---

## 11. Quick Q&A cheat cards (short answers)

**Q: What is the product?**  
A: Correctness engine for a Raft KV — seed, digest, minimized schedule — not “HA cloud database.”

**Q: What is `step`?**  
A: `fn step(node, event) -> Vec<Effect>` — protocol transition; interpreters apply effects.

**Q: Sim vs node?**  
A: Sim: we own time/net/disk/RNG. Node: OS owns files; v1 single-node WAL demo.

**Q: What’s in v1?**  
A: Election+replication, static 3/5, Get/Put via log, idempotency, swarm/replay/minify/coverage, 5 Raft safety + persist rules + linearizability, PLANTED pack, CI green.

**Q: Coolest bug?**  
A: Seed 281 CHECKER + heartbeat ReplayBook; PLANTED ElectionSafety as harness proof. No wild PROTOCOL pack.

**Q: Raft paper safety you check?**  
A: Five Raft safety properties (election/log safety family as implemented in Chronos checkers) plus persist-before-send/ack and linearizability. If asked for names, speak **Raft paper** terms: Election Safety, Log Matching, Leader Completeness, State Machine Safety, and related — then say “we encode five safety checks in-sim; I can walk the checker list from the repo if we open it.”

**Q: Why Get via log?**  
A: So reads participate in the same commit/apply story the linearizability checker sees — not leader-memory shortcuts (read-index is OUT).

---

## 12. Day-before checklist

- [ ] Recite **60s pitch** twice without notes  
- [ ] Recite **5-minute** structure with OUT list once  
- [ ] Full script §5.1–5.6 once on a recording (listen for PROTOCOL overclaims)  
- [ ] Whiteboard §7 from memory in ≤10 minutes  
- [ ] Seed 281 in ≤90s: heartbeat book → Io unknown → CLASS=CHECKER  
- [ ] PLANTED in ≤60s: skip_vote_persist test-only; swarm never enables  
- [ ] Answer “Why Rust?” with learning honesty (§9)  
- [ ] Answer “What next?” with 2 OUT items becoming IN (§10)  
- [ ] Skim F5 war stories once  

---

## 13. Read next

- Files and flows → [F2-files-and-code-flow.md](F2-files-and-code-flow.md)  
- Architecture locks → [F3-architecture-and-design.md](F3-architecture-and-design.md)  
- Rust for this repo → [F4-rust-for-chronos.md](F4-rust-for-chronos.md)  
- Problems / solutions → [F5-problems-and-solutions.md](F5-problems-and-solutions.md)  
- Question bank → [F6-interview-question-bank.md](F6-interview-question-bank.md)  
