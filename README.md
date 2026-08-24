# Chronos

**Simulation-first Raft key-value store — a correctness lab, not “yet another Raft clone.”**

[![CI](https://github.com/sahilahmed21/Chronos/actions/workflows/ci.yml/badge.svg)](https://github.com/sahilahmed21/Chronos/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Same protocol code runs under a **deterministic simulator** (we own time, network, disk, entropy) and a **production-style node** (OS owns files). When safety breaks, you get a **seed**, a **SHA-256 digest**, and a **minimized schedule** — not a flake.

**Repo:** https://github.com/sahilahmed21/Chronos

---

## At a glance (recruiters / hiring managers)

| | |
|--|--|
| **What** | Deterministic simulation testing (DST) harness around a minimal Raft KV |
| **Why** | Catch schedule-dependent distributed bugs with replayable artifacts |
| **Stack** | Rust 2021 · 3 crates · no Tokio under the protocol · Linux GitHub Actions |
| **v1 status** | Complete per [docs/roadmap/deliverable.md](docs/roadmap/deliverable.md) |
| **Proof** | CI green · 1000-seed swarm green on Actions · PLANTED ElectionSafety pack · hunt seed 281 closed as CHECKER |

```text
                    chronos-sim                 chronos-node
                 (scheduler + faults)         (single-node WAL demo)
                              \                      /
                               v                    v
                            chronos-protocol
                     fn step(node, event) -> Vec<Effect>
```

**Honest scope:** v1 is a correctness engine + single-node WAL demo. It is **not** a multi-node TCP HA cluster, Jepsen suite, or Antithesis run. Prefer that honesty in interviews — see [docs/interview/](docs/interview/README.md).

---

## Proof of results (clickable)

| Evidence | What it shows | Link / location |
|----------|---------------|-----------------|
| **CI `verify`** | fmt · release tests · clippy · determinism gates · swarm | [Actions / CI](https://github.com/sahilahmed21/Chronos/actions/workflows/ci.yml) |
| **1000-seed swarm (GitHub)** | Full nightly-scale batch, `# coverage runs=1000`, exit 0 | [Run 32767856965](https://github.com/sahilahmed21/Chronos/actions/runs/32767856965) |
| **PLANTED pack** | Skipping `votedFor` persist fails Election Safety (labeled harness proof) | [docs/bugs/2026-08-24-planted-skip-vote-persist/](docs/bugs/2026-08-24-planted-skip-vote-persist/) |
| **Hunt seed 281** | Swarm Linearizability fail → **CHECKER** fix (`Io` = unknown), not a fake Raft PROTOCOL win | [HUNT-2026-08-24.md](docs/bugs/HUNT-2026-08-24.md) · [CLOSE-281.md](docs/bugs/CLOSE-281.md) |
| **Deliverables sheet** | Checklist + SHAs + what we do / don’t claim | [docs/interview/F7-deliverables-and-proofs.md](docs/interview/F7-deliverables-and-proofs.md) |

Planted pack code pin (`COMMIT` file): `2b46ebb0c4216ead35a52352460a372e85ffae98` (intentional pin; not always `HEAD`).

---

## Quick demo (5 minutes)

Requires Rust (`rustup`) or Docker.

### Option A — Rust toolchain

```bash
git clone https://github.com/sahilahmed21/Chronos.git
cd Chronos

cargo test --workspace --release
cargo run -p chronos-sim --release -- --seed 7
```

You should see a deterministic **digest** for seed `7`. Run the same command again → **same digest**.

### Option B — Docker (Windows without MSVC `link.exe`)

```bash
docker run --rm -v c:/projects/Chronos:/work -w /work -e RUST_TEST_THREADS=1 \
  rust:1-bookworm cargo test --workspace --release

docker run --rm -v c:/projects/Chronos:/work -w /work rust:1-bookworm \
  cargo run -p chronos-sim --release -- --seed 7
```

Use **PowerShell** for `c:/…` mounts. Git Bash often breaks `-v` (`invalid mode: /work`).

### Demo: swarm + coverage

```bash
mkdir -p traces
cargo run -p chronos-sim --release -- --seeds 32 --start 1 --out traces --coverage
```

Exit `0` if all seeds pass. `--coverage` prints profile knobs vs faults that **actually fired** (not “all combinations explored”).

### Demo: PLANTED ElectionSafety (harness proof)

```bash
cargo test -p chronos-sim --test properties planted_skip_vote_persist_fails_election_safety -- --exact
cargo test -p chronos-sim --test properties unplanted_solo_campaign_passes_persist_before_send -- --exact
```

Planted tests trip Election Safety / persist-before-send **on purpose**. Unplanted control stays clean. Swarm never enables the hook.

### Demo: single-node WAL node

```bash
cargo run -p chronos-node --release
```

Writes a local WAL and drives Put/Get through the **same** `step` function (solo node, not a 3-process TCP cluster).

---

## How to use (CLI cheatsheet)

| Goal | Command |
|------|---------|
| One seed | `cargo run -p chronos-sim --release -- --seed <S>` |
| Batch swarm | `cargo run -p chronos-sim --release -- --seeds N --start 1 --out traces --coverage` |
| Replay fail file | `cargo run -p chronos-sim --release -- --replay traces/fail-<S>.trace` |
| Minimize schedule | `cargo run -p chronos-sim --release -- --minify --seed <S> --out traces` |
| Workspace tests | `cargo test --workspace --release` |
| Determinism gates | `bash scripts/check-determinism-gates.sh` |

**CI swarm sizes:** push/PR → **32** seeds · schedule (`0 6 * * *` UTC) or `workflow_dispatch` with `swarm_seeds=1000` → **1000** seeds.

---

## How it works (short)

1. **Protocol** (`chronos-protocol`) — pure `step(state, event) → effects`. No clocks, sockets, files, or `HashMap`.
2. **Simulator** (`chronos-sim`) — one thread, tickless `u64` ns, heap `(time, global seq)`, seeded RNG, fault injection, white-box Raft checks + linearizability, swarm/minify.
3. **Node** (`chronos-node`) — interprets the same effects against a real append-only WAL.

**Invariants checked:** Election Safety · Leader Append-Only · Log Matching · Leader Completeness · State Machine Safety · persist-before-send/ack · client linearizability.

**Disk model:** process buffer vs `durable_len`; torn suffix only past durable; fsync-lie modeled but **default off**.

Full design: [docs/02-architecture.md](docs/02-architecture.md) (D1–D17 wins all conflicts).

---

## What’s in / out (v1)

**In:** DST harness · Raft election/replication · Get via log · idempotent client ops · swarm fuzz · minify · PLANTED regressions · Linux CI · coverage table.

**Out:** Byzantine · dynamic membership · snapshots · read-index/leases · multi-DC clocks · Jepsen · fsync-lie default-on · Antithesis of an unmodified binary · multi-node TCP HA (node is single-process WAL demo).

---

## Docs map

| Doc | For |
|-----|-----|
| [docs/interview/](docs/interview/README.md) | **Interview pack F1–F7** (pitch, code flow, architecture, Rust, war stories, Q bank, deliverables/proofs) |
| [docs/interview/F7-deliverables-and-proofs.md](docs/interview/F7-deliverables-and-proofs.md) | Evidence sheet (checklists, SHAs, Actions) |
| [docs/02-architecture.md](docs/02-architecture.md) | Constitution |
| [docs/roadmap/deliverable.md](docs/roadmap/deliverable.md) | v1 definition of done |
| [docs/roadmap/goals.md](docs/roadmap/goals.md) | G0–G11 |
| [docs/bugs/](docs/bugs/README.md) | PLANTED pack + hunt logs |

---

## License

MIT — [LICENSE](LICENSE). Rust edition 2021.
