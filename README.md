# Chronos

A **simulation-first Raft key-value store**. The same protocol runs under a deterministic simulator (time, network, disk, and entropy are controlled) and under a production-style node (the OS owns files). Failures produce a **seed**, a **SHA-256 trace digest**, and a **minimized fault schedule** that can be replayed — not a one-off flake.

[![CI](https://github.com/sahilahmed21/Chronos/actions/workflows/ci.yml/badge.svg)](https://github.com/sahilahmed21/Chronos/actions/workflows/ci.yml)

```text
                    chronos-sim                 chronos-node
                 (scheduler + faults)         (single-node WAL)
                              \                      /
                               v                    v
                            chronos-protocol
                     fn step(node, event) -> Vec<Effect>
```

| | |
|--|--|
| **Language** | Rust 2021 |
| **Layout** | Three crates: `chronos-protocol`, `chronos-sim`, `chronos-node` |
| **Core API** | Synchronous `step` — no Tokio under the protocol |
| **Verification** | Continuous integration on Linux; swarm fuzz on every push |

---

## What it does

Chronos treats correctness as a product: the Raft + KV state machine never touches the wall clock, sockets, or the filesystem. Interpreters apply `Effect`s. The simulator explores schedules with a seeded PRNG, checks Raft safety and linearizability, and dumps reproducible artifacts on failure.

**Checked invariants:** Election Safety · Leader Append-Only · Log Matching · Leader Completeness · State Machine Safety · persist-before-send / persist-before-ack · client-history linearizability.

**v1 includes:** deterministic DST harness, static-cluster Raft, Get/Put via the log, swarm fuzz, schedule minify, coverage reporting, planted persist regressions, Linux CI.

**v1 does not include:** dynamic membership, snapshots, read-index, multi-node TCP HA, Jepsen, Antithesis, or fsync-lie-by-default. `chronos-node` is a single-process WAL demo using the same protocol crate.

Design reference: [docs/02-architecture.md](docs/02-architecture.md).

---

## Build and test

```bash
git clone https://github.com/sahilahmed21/Chronos.git
cd Chronos

cargo build --workspace --release
cargo test --workspace --release
cargo clippy -p chronos-protocol -p chronos-sim --all-targets -- -D warnings
```

Without a local MSVC linker on Windows, use Docker (PowerShell; forward slashes on the mount):

```bash
docker run --rm -v c:/projects/Chronos:/work -w /work -e RUST_TEST_THREADS=1 \
  rust:1-bookworm cargo test --workspace --release
```

Determinism source gates (no `HashMap` in protocol/sim; no host I/O types in protocol):

```bash
bash scripts/check-determinism-gates.sh
# Windows: powershell -NoProfile -File scripts/check-determinism-gates.ps1
```

---

## Demo

### Deterministic seed

```bash
cargo run -p chronos-sim --release -- --seed 7
```

Prints a SHA-256 digest for the run. Running the same seed again yields the **same** digest.

### Swarm batch

```bash
mkdir -p traces
cargo run -p chronos-sim --release -- --seeds 32 --start 1 --out traces --coverage
```

Exit code `0` if every seed passes. On failure, writes `traces/fail-<seed>.trace` (and a planned schedule). `--coverage` reports configured profile knobs separately from faults that actually fired.

### Replay and minify

```bash
cargo run -p chronos-sim --release -- --replay traces/fail-<seed>.trace
cargo run -p chronos-sim --release -- --minify --seed <seed> --out traces
```

### Planted Election Safety regression

```bash
cargo test -p chronos-sim --test properties planted_skip_vote_persist_fails_election_safety -- --exact
cargo test -p chronos-sim --test properties unplanted_solo_campaign_passes_persist_before_send -- --exact
```

The planted hook is test-only and is never enabled in swarm or CLI minify. Pack: [docs/bugs/2026-08-24-planted-skip-vote-persist/](docs/bugs/2026-08-24-planted-skip-vote-persist/).

### Single-node WAL

```bash
cargo run -p chronos-node --release
```

Persists to a local WAL file and serves Put/Get through the same `step` function (one process; not a multi-node TCP cluster).

---

## CLI reference

| Goal | Command |
|------|---------|
| One seed | `cargo run -p chronos-sim --release -- --seed <S>` |
| Batch | `cargo run -p chronos-sim --release -- --seeds N --start 1 --out traces --coverage` |
| Replay | `cargo run -p chronos-sim --release -- --replay traces/fail-<S>.trace` |
| Minify | `cargo run -p chronos-sim --release -- --minify --seed <S> --out traces` |
| Tests | `cargo test --workspace --release` |

CI runs **32** seeds on push/PR and **1000** seeds on the nightly schedule (and on manual `workflow_dispatch` with `swarm_seeds=1000`). Workflow: [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

---

## Architecture (brief)

1. **`chronos-protocol`** — `step(state, event) → Vec<Effect>`. No clocks, sockets, files, threads, or `HashMap`.
2. **`chronos-sim`** — single-threaded tickless scheduler (heap key `(time, global seq)`), seeded RNG, simulated net/disk, checkers, swarm, minify.
3. **`chronos-node`** — interprets the same effects against a real append-only WAL (`[len][crc32][payload]`).

Disk model: process-visible buffer vs durable watermark; torn writes only past the durable prefix; fsync-lie is modeled but off by default.

---

## Documentation

| Document | Contents |
|----------|----------|
| [docs/02-architecture.md](docs/02-architecture.md) | Locked design (D1–D17) |
| [docs/roadmap/](docs/roadmap/README.md) | Goals and phase sequence |
| [docs/roadmap/deliverable.md](docs/roadmap/deliverable.md) | v1 definition of done |
| [docs/bugs/](docs/bugs/README.md) | Finding packs and hunt notes |

---

## License

MIT — see [LICENSE](LICENSE).
