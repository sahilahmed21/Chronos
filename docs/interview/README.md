# Interview prep index — Chronos

**Audience:** ~30 LPA+ backend interviews. Chronos is a **core resume project**.  
**Assumption:** you may know **zero Rust** and have **no prior Chronos context**. Study F1→F6 in order.

| File | Purpose | Size (approx) |
|------|---------|---------------|
| [F1-project-pitch-and-script.md](F1-project-pitch-and-script.md) | Explain the project + full spoken interview script | ~23 KB |
| [F2-files-and-code-flow.md](F2-files-and-code-flow.md) | Which file is what + Put/election/crash/swarm/minify flows | ~22 KB |
| [F3-architecture-and-design.md](F3-architecture-and-design.md) | How it works, D1–D17, checkers, whiteboard 15 min | ~21 KB |
| [F4-rust-for-chronos.md](F4-rust-for-chronos.md) | Rust only as it appears in Chronos (answer Rust Qs near this topic) | ~26 KB |
| [F5-problems-and-solutions.md](F5-problems-and-solutions.md) | Real problems we hit and how we fixed them | ~20 KB |
| [F6-interview-question-bank.md](F6-interview-question-bank.md) | Fundamentals, live coding, scenarios, traps, scored mock | ~34 KB |
| [F7-deliverables-and-proofs.md](F7-deliverables-and-proofs.md) | **What we shipped + evidence** (checklist, SHAs, Actions URLs, PLANTED/281) | evidence sheet |

**Companion (deeper, not grilling):**

- `docs/02-architecture.md` — constitution (wins all conflicts)
- `docs/00-project-guide.md` §8 — long-form Raft/DST Q&A
- `docs/bugs/` — PLANTED pack + hunt 281 CLOSED
- `docs/HANDOFF.md` — current pins / locks

---

## Source of truth (do not invent)

| Source | Role |
|--------|------|
| `docs/02-architecture.md` | D1–D17; wins conflicts |
| `docs/roadmap/deliverable.md` | v1 done definition |
| `docs/bugs/2026-08-24-planted-skip-vote-persist/` | G9 PLANTED |
| `docs/bugs/HUNT-2026-08-24.md` / `CLOSE-281.md` | Seed 281 → CHECKER |
| GitHub `sahilahmed21/Chronos` | CI evidence |

---

## Honest claims (memorize before every interview)

**Do claim**

- Simulation-first Raft KV; same `step` under sim and node
- Deterministic swarm; SHA-256 digests; replay/minify tooling
- Five Raft safety + persist-before-send/ack + linearizability
- PLANTED ElectionSafety harness proof (labeled)
- Seed 281 closed as **CHECKER** (`Io` as unknown) + heartbeat recording (HARNESS)
- Linux CI green; GitHub **1000-seed** dispatch green (see F7)

**Do not claim**

- Wild PROTOCOL Raft safety bug from swarm
- Multi-node TCP HA / 3-process production cluster
- Jepsen / Antithesis shipped
- Fsync-lie default-on; snapshots; membership; read-index
- “HashMap is fine with a fixed hasher” in protocol or sim

---

## Suggested study plan (5–7 evenings)

| Day | Focus |
|-----|--------|
| 1 | **F1** until you can deliver 60s + 5 min without notes |
| 2 | **F3** + draw hexagon + disk model on paper |
| 3 | **F2** with repo open; narrate Put and election aloud |
| 4 | **F5** war stories (281, drain, election RTT) |
| 5 | **F4** Rust Qs mapped to Chronos |
| 6 | **F6** mock 30 min (record yourself); fail if you say PROTOCOL for 281 |
| 7 | **F7** deliverables + proof URLs/SHAs (print or keep open in interview) |
| 8 | Red flags + resume bullets + “what next” |

---

## Mock interview (30 min)

Use F6 section G (scored mock). Record yourself. Instant fail if you call seed 281 a PROTOCOL bug or claim multi-node TCP HA.
