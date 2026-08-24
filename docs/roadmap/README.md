# Roadmap

How Chronos gets from empty crates to a portfolio-grade DST product.

If this folder and [../02-architecture.md](../02-architecture.md) disagree, **the architecture wins**. This folder only sequences work; it does not change D1–D17.

## How to read

| File | What it is |
|------|------------|
| [goals.md](goals.md) | Ultimate goal, sub-goals, acceptance, mapping to phases |
| [deliverable.md](deliverable.md) | What “v1 shipped” looks like (artifacts, not vibes) |
| [P01-foundation.md](P01-foundation.md) … [P09-product.md](P09-product.md) | Subphases → steps → substeps → exit checks |

IDs:

```text
G1 / G1.1          goal / sub-goal
P1                 phase
P1.A               subphase
P1.A.1             step
P1.A.1.a           substep
```

A phase is **done** only when its exit checks pass. Do not start the next phase early. In particular: **no Raft in P1, no Tokio in P2.**

## Sequence

```text
P1 Foundation  →  P2 Simulator  →  P3 Raft
        ↓                ↓              ↓
   swap interpreters   same seed     happy path
                       = same hash    3-node KV

P4 Faults  →  P5 Properties  →  P6 Fuzz  →  P7 Bug
     ↓              ↓               ↓           ↓
  scripted       planted         swarm       seed +
  scenarios      votedFor        seeds       trace

P8 Minify  →  P9 Product
     ↓              ↓
  smallest       CI + README
  repro          + coverage
```

P7 is the punchline. P8 makes it readable. P9 makes it a product. P1–P6 exist so P7 is possible.

## Current state

**P1–P8 tooling implemented** (protocol, sim, Raft, faults, checkers, swarm, replay, minify).  
**P9 product** in progress / landing: README, Linux CI, `--coverage`, PLANTED bug pack linked.  
Local Windows may lack `link.exe`; Linux CI is the required green gate. Do not claim G7–G10 green from clippy alone.

Next: keep CI green; fill `COMMIT` after `git init`; optional wild PROTOCOL pack if swarm finds one.
