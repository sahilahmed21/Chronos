# F5 — Problems and solutions (Chronos war stories)

**Audience:** ~30 LPA+ backend interviews. Chronos is your core resume project.  
**Goal:** Drill-ready Problem → Symptom → Root cause → Fix → Interview line for real Chronos history only.  
**Source of truth:** This briefing + common Raft knowledge labeled **Raft paper**. Spec `docs/02-architecture.md` wins.  
**Hard rule:** Do **not** invent PROTOCOL bugs. Prefer wild PROTOCOL **not found**. Seed 281 = CHECKER. G9 = PLANTED (design/harness), not a wild find.

Repo: https://github.com/sahilahmed21/Chronos  

---

## 0. How to use this file

1. Memorize the **CLASS taxonomy table** (§1) before any war story.  
2. Practice each problem’s **Interview line** out loud until it is under 45–60 seconds.  
3. Practice the **hardest bug** one-minute answer (§12) until you never say “PROTOCOL” for 281.  
4. Before interviews, re-read **What NOT to claim** (§13).

---

## 1. CLASS taxonomy (memorize)

When swarm or a test fails, classify before you celebrate.

| CLASS | Meaning | Chronos example | Interview claim allowed? |
|-------|---------|-----------------|--------------------------|
| **HARNESS** | Recording, replay, minify, test glue wrong — sim world not faithfully replayed | Heartbeat jitter missing from ReplayBook → MINIFY MISMATCH on seed 281 | Yes — “recording gap” |
| **CHECKER** | Oracle / property check wrong — false fail or false pass | Seed 281: Io treated as definite miss | Yes — “oracle bug” |
| **SIM** | Simulator world model bug (time/net/disk/RNG scheduling) | (Prefer not to invent; only claim if you have a real pack) | Only with evidence |
| **PROTOCOL** | Raft/KV `step` logic violates safety or promised semantics | Prefer **not found** wild; do not invent | Only with a real PROTOCOL pack — Chronos v1 portfolio does **not** claim one |
| **PLANTED** | Intentional fault injection / hook to prove checkers | `skip_vote_persist` ElectionSafety pack | Yes — labeled **PLANTED**, never “wild find” |
| **CI / DEVEX** | Toolchain, timeouts, clippy, drain modes, flaky runners | drain hang; clippy `as_chunks`; CI path | Yes — engineering maturity |

**Triage order (say this):** HARNESS → CHECKER → SIM → PROTOCOL.  
Never enable `skip_vote_persist` to “force” a minify or a PROTOCOL story.

---

## 2. War-story formula (every problem below)

1. **Problem** — name the issue in one clause  
2. **Symptom** — what you saw fail  
3. **Root cause** — one precise cause  
4. **Fix** — what changed  
5. **Interview line** — spoken punchline  

Optional sixth beat if asked: **What I’d do differently.**

---

## 3. Seed 281 — Linearizability CHECKER (false fail)

### Problem

1000-seed swarm Linearizability failure that looked like a KV/Raft apply bug but was an **oracle** mistake.

### Symptom

- Swarm: seed **281**, check **Linearizability** failed.  
- Replay of the seed could reproduce the failure path.  
- Client history pattern of interest: **Put** completed with **`Err(Io)`**, later **Get** returned **`Ok(value)`** for that key/value.  
- Easy wrong story: “Put failed but value appeared — Raft is broken.”

### Root cause

`ClientError::Io` was treated as a **definite miss** (as if the put never happened).  
Architecture / black-box rule: after a possible apply, **Io is unknown** — the operation may have applied **or** been skipped.  
Treating Io as definite miss made a later Get of the applied value look **illegal** → **false** Linearizability fail.  
**CLASS=CHECKER.**

### Fix

Search **Io** as **unknown** (apply **or** skip). Keep definite errors (e.g. NotLeader / Invalid style) as definite where that is correct.  
Seed 281 closed as **CHECKER**. After related harness fix (§4), 1000-seed **Docker** + **GitHub dispatch** green.

### Interview line

> Seed 281 failed Linearizability in a thousand-seed swarm. The history was Put returning Io, then Get returning the value. Our checker treated Io like a definite miss. That’s wrong when the put might already have applied — Io has to stay unknown, apply or skip. We fixed the oracle, reclassified as CHECKER, and did not ship a fake Raft PROTOCOL bug.

### If they ask “Is unknown the same as success?”

> No. Unknown means the sequential spec must consider **both** apply and skip branches. Definite miss only allows the skip branch. Mixing those up creates false fails — exactly 281.

---

## 4. Seed 281 — Heartbeat ReplayBook (MINIFY MISMATCH)

### Problem

Minify could not faithfully replay the failing schedule because heartbeat delays were not in the book.

### Symptom

- `--minify` (or equivalent minify path) reported **MINIFY MISMATCH**.  
- Full recorded schedule did not reproduce the live failure the way a correct recording should.  
- Live path used heartbeat delay **plus jitter**; recorded path effectively lost that entropy (jitter not booked).

### Root cause

**ReplayBook** recorded some delay families (e.g. send / io / election style delays) but **not heartbeat** delays.  
Live vs Recorded diverged at heartbeat **TimerFired** scheduling under jitter.  
**CLASS=HARNESS** (recording gap). This blocked trusting minify until fixed — and must be told **before** or **alongside** the checker story so you show triage discipline.

### Fix

Record **heartbeat** delays in ReplayBook with **FIFO** semantics (same idea as election delay recording — keyed consistently per node/life).  
After FIFO heartbeat binding, minify could agree with live (mismatch resolved), and triage could proceed to the real CHECKER root cause (§3).

### Interview line

> Before we blamed Raft, minify mismatched. Heartbeat jitter wasn’t in the ReplayBook, so Recorded was a different world than Live. We fixed heartbeat FIFO recording first. Only then did the Linearizability failure cleanly classify as a checker bug. Recording is part of deterministic simulation — if you can’t replay, you don’t have a pack.

### Drill order for interviews

Always tell **HARNESS then CHECKER** for 281, not the reverse. It proves you do not jump to PROTOCOL.

---

## 5. Drain hang — `drain` vs `drain_horizon` (CI)

### Problem

CI / workspace tests hung or burned the whole simulation window on idle heartbeats.

### Symptom

- Tests cancelled or timed out on CI.  
- Debug builds especially painful: `drain()` appeared to run until `max_ns` on heartbeat timers.  
- Idle cluster with a live leader still had timers on the heap; “drain everything” never looked quiescent in the way tests needed.

### Root cause

One drain API was doing **horizon** work (run until time budget) when tests wanted **quiescence** (stop when the interesting work is done).  
Idle leaders keep **heartbeats** scheduled. Treating that like “keep draining forever toward max_ns” hangs CI.  
Need **two modes**.

### Fix

- **`drain()`** — stop when there is a **live leader** and the heap has **no non-timer** events (quiescent enough for tests).  
- **`drain_horizon()`** — used by swarm / minify when you **want** the full horizon so late extras still fire.  
- CI practicalities: release tests, single-thread test runs where needed, generous timeouts — as part of the green CI path (§10).

### Interview line

> We had CI hangs because drain meant two different things. Tests want quiescence — stop when there’s a leader and only timers left. Fuzz wants the full horizon. Splitting `drain` and `drain_horizon` fixed hangs without blinding the swarm.

---

## 6. Commit advances without followers — `maybe_commit` → `replicate_all`

### Problem

Leader commit index moved forward but followers did not learn `leaderCommit` promptly.

### Symptom

- After a short `drain()`, Put/Get progress could stall in scenarios that expected followers to catch commit.  
- Leader believed commit advanced; followers were behind on commit awareness.

### Root cause

Idle **`drain()`** can stop **before** the next heartbeat.  
If commit advances and you **do not** replicate, followers miss `leaderCommit` until a later heartbeat that may never run inside that short drain.  
**Raft paper** reminder: followers learn commit through AppendEntries (including heartbeats carrying `leaderCommit`). Relying only on the next timed heartbeat is fragile under quiescent drain.

### Fix

When **`maybe_commit`** advances commit, call **`replicate_all`** so followers are pushed the new commit without waiting for the next heartbeat timer.

### Interview line

> We hit a case where commit moved on the leader but followers never saw it under short drain — drain can stop before the next heartbeat. The fix was: when maybe_commit advances, replicate_all immediately. Don’t assume a heartbeat will run just because Raft would eventually send one.

---

## 7. Delayed-message election vs RTT

### Problem

A delayed-network election scenario never elected a leader before the simulation budget.

### Symptom

- Scenario with substantial message delay (order of hundreds of ms each way).  
- No leader by `max_ns`.  
- Looks like “Raft election is broken” if you do not compute RTT vs timeout.

### Root cause

RequestVote **RTT** was about **800ms** (e.g. ~400ms each way).  
Election timeout was on the order of **~800–900ms** — roughly **one RTT**.  
Votes could not round-trip before the election timed out again; the heap burned time without a stable leader.  
**Raft paper** reminder: election timeouts must be longer than typical broadcast / RTT, or elections thrash.

### Fix

Raise election timeout to **2.0–2.5s** (**greater than** the 800ms RTT).  
Extend `max_ns` as needed (e.g. tens of seconds for the scenario).  
Use a **`wait_leader()`**-style helper that steps until a leader exists instead of assuming a blind short `drain()` after recover.

### Interview line

> We had a delayed-message test that never elected. Vote RTT was about 800 milliseconds and election timeout was about the same — one RTT. Timeouts must be larger than RTT. We moved election to 2.0–2.5 seconds and waited for a leader explicitly. That’s a systems timing bug, not a mysterious Raft safety failure.

---

## 8. SHA-256 round constant typo

### Problem

Trace digest unit tests failed known SHA-256 vectors.

### Symptom

- Digest tests against known vectors were red.  
- Cross-platform “same seed → same digest” story was at risk until fixed.

### Root cause

SHA-256 round constant typo: wrong **`0x14292977`** vs correct **`0x14292967`**.

### Fix

Correct the constant in the digest implementation (`trace` path). Digests must match known vectors so swarm artifacts are trustworthy across machines.

### Interview line

> Our SHA-256 known-vector tests failed because of a one-nibble typo in a round constant — 0x14292977 instead of 0x14292967. Digests are the identity of a swarm failure; if SHA-256 is wrong, every artifact is meaningless. Easy bug, high leverage fix.

---

## 9. PLANTED design — `skip_vote_persist` (not a wild bug)

### Problem

*(Framed carefully: this is a **design/harness** story, not an accidental production bug.)*  
How do you prove ElectionSafety and persist-before-send oracles work without lying about a wild PROTOCOL find?

### Symptom

- With the **test-only** hook `skip_vote_persist` enabled on a node, **Election Safety** fails under crash / partition schedules.  
- Without the hook, persist-before-send paths hold.

### Root cause (by design)

If you **send or grant votes before Meta (`votedFor` / term) is durable**, a crash can reboot and vote again in the **same term** → two leaders in one term.  
Violates **Election Safety** (**Raft paper**: at most one leader per term).  
This is the failure mode the plant demonstrates.

### Fix / packaging (engineering response)

- Hook is **test-only**.  
- **Swarm never enables it.**  
- Pack lives at `docs/bugs/2026-08-24-planted-skip-vote-persist/` with **CLASS=PLANTED**.  
- G9 portfolio artifact: harness proof for ElectionSafety + persist-before-send story.  
- Prefer wild PROTOCOL **not found** — do not rebrand this plant as wild.

### Interview line

> Our G9 artifact is intentionally PLANTED. A test-only skip_vote_persist hook shows that if you vote before Meta is durable, Election Safety fails after crash. Swarm never turns that hook on. I will not call that a wild PROTOCOL discovery — labeling honesty is part of the engineering.

### If they say “So DST never found anything real?”

> DST found seed 281 — a real CHECKER bug and a real HARNESS recording gap. Those are real. What we refuse to do is inflate a plant or a checker false fail into a PROTOCOL resume line.

---

## 10. CI path — green gates (and related polish)

### Problem

“Tests passed on my laptop” is not the product. Chronos needs a **repeatable CI path** that stays green under clippy and swarm.

### Symptom

- CI red after local green (clippy `-D warnings` on newer Rust).  
- Example lint: prefer **`as_chunks`** over some chunk patterns.  
- Swarm / release test configuration must actually run (including 1000-seed confirmation after 281).  
- Related correctness test polish: **buggify** AppendEntries nack test failed when asserting NACK too early — higher-term AE persists **Meta first**; drain term persist / fsync **before** NACK assert.

### Root cause

- Clippy and toolchains move; ignoring CI clippy is a false green.  
- Persist ordering in Raft (**Raft paper** + Chronos persist-before-send/ack discipline) means tests must wait for Meta durability effects before observing protocol replies.  
- Drain/swarm CI settings interact with §5.

### Fix

- Match CI clippy (including `as_chunks` style fixes).  
- Keep CI green: fmt / release tests / clippy / determinism gates / swarm (including **1000-seed Docker + GitHub dispatch** green after 281 close).  
- Buggify AE nack: persist Meta first; assert NACK only after that drain.

### Interview line

> CI is part of the correctness product. We don’t claim green from a single local test run. Clippy gates caught churn like as_chunks. After seed 281 we reconfirmed with a thousand-seed Docker run and GitHub dispatch. Even buggify tests taught persist-before-observe — Meta first, then NACK assert.

---

## 11. Quick reference table (all drill items)

| # | Problem | CLASS | One-line cause | One-line fix |
|---|---------|-------|----------------|--------------|
| 1 | Seed 281 Linearizability | CHECKER | Io treated as definite miss | Io = unknown (apply or skip) |
| 2 | Seed 281 minify mismatch | HARNESS | Heartbeat jitter not in ReplayBook | Heartbeat FIFO in book |
| 3 | CI / test hang | CI / DEVEX | `drain` ≈ horizon on heartbeats | `drain` vs `drain_horizon` |
| 4 | Followers miss commit | PROTOCOL-*adjacent* fix in leader path* | Commit without replicate under short drain | `maybe_commit` → `replicate_all` |
| 5 | No election under delay | SIM/scenario timing | Election timeout ≈ 800ms RTT | Election **2.0–2.5s** > RTT; wait_leader |
| 6 | SHA-256 vectors | DEVEX / correctness infra | Constant `…977` vs `…967` | Use **0x14292967** |
| 7 | ElectionSafety demo | PLANTED | Vote before Meta durable | Test-only hook; pack; swarm off |
| 8 | CI path / buggify / clippy | CI | Toolchain + assert ordering | Clippy green; Meta before NACK; 1000-seed green |

\*Item 4 is a real leader-path fix worth telling, but it is **not** a wild swarm PROTOCOL safety pack. Do not upgrade it into “we found Election Safety broken in prod.”

---

## 12. Spoken drills

### 12.1 One-minute “hardest bug” (memorize)

> Seed 281 failed Linearizability in a 1000-seed swarm. Replay worked; minify mismatched because heartbeat jitter wasn’t in the ReplayBook — a harness recording gap we fixed with heartbeat FIFO. After that, the real bug was the checker: Put returned Io, Get later returned the value, and the oracle treated Io as a definite miss. Architecture says Io after a possible apply is unknown — apply or skip. We fixed the oracle, closed 281 as CHECKER, confirmed thousand-seed Docker and GitHub dispatch green, and we did **not** invent a PROTOCOL Raft finding.

### 12.2 Thirty-second PLANTED

> G9 is a PLANTED pack: skip_vote_persist is test-only, swarm never enables it. Voting before Meta is durable breaks Election Safety after crash. That proves the checker. I label it PLANTED; I prefer wild PROTOCOL not found.

### 12.3 Forty-second “systems debugging” medley

> Day-to-day Chronos bugs were often timing and harness: drain versus drain_horizon for CI hangs; maybe_commit calling replicate_all so followers see commit without waiting for a heartbeat; election 2.0–2.5 seconds when RTT was 800 milliseconds; a SHA-256 constant typo; clippy as_chunks for CI. Those stories show debugging discipline, not portfolio inflation.

### 12.4 If interrupted with “So did Raft have a bug?”

> Not a wild safety PROTOCOL pack from swarm. We had a PLANTED ElectionSafety demonstration, a CHECKER false fail on 281, harness recording gaps, and several sim/CI timing fixes. I’m careful with that word “bug” because interviewers hear “Raft safety violated in production code.”

---

## 13. What NOT to claim

Do **not** say:

- “We found a wild PROTOCOL Raft safety bug in Chronos via swarm.”  
- “Seed 281 proves Raft applied wrongly.”  
- “PLANTED skip_vote_persist runs in swarm / production.”  
- “Chronos ships multi-node TCP HA / Jepsen / Antithesis / snapshots / membership / read-index / Byzantine / Tokio-under-protocol.”  
- “Fsync-lie is on by default.”  
- “`chronos-node` is a production HA cluster.”  
- “Minify mismatch means the protocol is nondeterministic” (for 281 — it was heartbeat recording).  
- “Io means the put definitely did not happen.”  
- Any **invented** PROTOCOL bug narrative not backed by a real PROTOCOL pack.

**Do claim:**

- Simulation-first Raft KV; same `step` under sim and node.  
- Deterministic swarm; SHA-256 digests; minify tooling.  
- Five Raft safety + persist-before-send/ack + linearizability checkers.  
- PLANTED ElectionSafety pack (labeled).  
- Seed 281 closed CHECKER (+ heartbeat book HARNESS).  
- CI green; 1000-seed Docker + GitHub dispatch green after close.  
- The concrete fixes in this file (drain, replicate_all, election RTT, SHA-256 constant, clippy, buggify Meta ordering).

---

## 14. Cross-cutting lessons (senior signal)

1. **Triage order beats ego** — HARNESS / CHECKER before PROTOCOL.  
2. **Recording is part of DST** — no faithful ReplayBook ⇒ no trustworthy minify.  
3. **Oracles can lie** — linearizability mishandling Io is as dangerous as Raft bugs.  
4. **Label honesty** — PLANTED vs wild PROTOCOL is a trust signal.  
5. **Timing is a dependency** — election timeout vs RTT; drain vs horizon; commit vs replicate.  
6. **CI is product** — clippy, swarm sizes, dispatch confirmation.

---

## 15. Day-before checklist

- [ ] Recite CLASS table meanings without looking  
- [ ] 281 in ≤90s: HARNESS heartbeat → CHECKER Io unknown  
- [ ] PLANTED in ≤45s: test-only; swarm never enables  
- [ ] drain vs drain_horizon in ≤30s  
- [ ] maybe_commit → replicate_all in ≤30s  
- [ ] election 2.0–2.5s > 800ms RTT in ≤30s  
- [ ] SHA-256 `0x14292967` one-liner  
- [ ] Re-read §13 What NOT to claim  

---

## 16. Read next / related

- Pitch + script → [F1-project-pitch-and-script.md](F1-project-pitch-and-script.md)  
- Flows → [F2-files-and-code-flow.md](F2-files-and-code-flow.md)  
- Architecture → [F3-architecture-and-design.md](F3-architecture-and-design.md)  
- Question bank → [F6-interview-question-bank.md](F6-interview-question-bank.md)  
- Pack paths: `docs/bugs/2026-08-24-planted-skip-vote-persist/`, seed 281 close notes in repo bug docs when you open the tree  
