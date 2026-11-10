# Chronos

A **simulation-first Raft key-value store** built to demonstrate Deterministic Simulation Testing (DST) at the protocol level. The same Raft state machine runs under a deterministic simulator — where every source of nondeterminism (time, network, disk, entropy) is controlled — and under a production-style node that uses the real OS. Failures produce a **seed**, a **SHA-256 trace digest**, and a **minimized fault schedule** that can be replayed byte-for-byte.

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

Chronos treats correctness as a first-class product concern: the Raft + KV state machine never touches the wall clock, sockets, or filesystem directly. All I/O is expressed as `Effect`s interpreted by the runtime. The simulator explores schedules with a seeded PRNG, checks Raft safety and linearizability after every event, and dumps reproducible artifacts on failure.

**Checked invariants:** Election Safety · Leader Append-Only · Log Matching · Leader Completeness · State Machine Safety · persist-before-send / persist-before-ack · client-history linearizability.

**v1 includes:** deterministic DST harness, static-cluster Raft, Get/Put via the log, swarm fuzz, schedule minify, coverage reporting, planted persist regressions, Linux CI.

**v1 does not include:** dynamic membership, snapshots, read-index, multi-node TCP HA, Jepsen, Antithesis, or fsync-lie-by-default. `chronos-node` is a single-process WAL demo using the same protocol crate.

Design reference: [docs/02-architecture.md](docs/02-architecture.md).

---

## Results

- **Determinism:** `cargo run -p chronos-sim --release -- --seed 7` produces the same SHA-256 digest across runs on the same commit (no host paths or wall clock in the hash).
- **CI:** `ci / verify` on `main` runs format checks, `cargo test --workspace --release`, clippy (`-D warnings`), determinism gates, and a swarm batch — [workflow](https://github.com/sahilahmed21/Chronos/actions/workflows/ci.yml).
- **Swarm at scale:** a **1000-seed** run with coverage completed successfully on GitHub Actions ([example run](https://github.com/sahilahmed21/Chronos/actions/runs/32767856965); log reports `# coverage runs=1000`).
- **Harness proof:** skipping durable `votedFor` persist fails Election Safety; the unplanted control stays clean. The hook exists only in builds with `--features chronos-sim/mutants` ([ADR-0002](docs/adr/0002-feature-gated-mutants.md)); default builds panic if one is selected, and swarm/CLI minify never select one.
- **Oracle hardening:** a Linearizability failure on seed 281 was traced to treating fsync `Io` errors as definite misses when the put may already have applied. The checker now treats `Io` as unknown. After the fix, seed 281 and the 1000-seed batch are clean.


---

## Oracle detection power

Six deliberate protocol bugs ("mutants") are compiled in only with the `mutants` feature. Each was run through the swarm with the bug active on every node (`chronos-sim --mutant <name> --seeds N`, release build, WSL2 Ubuntu, 2026-11-10, `--start 0`). The unmutated code stays clean over the same seeds (the 1000-seed CI swarm, plus the test `clean_swarm_stays_clean_with_the_feature_compiled_in`).

| Mutant | Bug | Seeds run | Detected | First seed | Check that fired |
|---|---|---|---|---|---|
| `skip-vote-persist` | vote sent/granted before term+votedFor fsync | 2,000 | 2,000 | 0 | PersistBeforeSend |
| `ack-before-fsync` | follower acks AppendEntries before fsync | 2,000 | 1,580 | 0 | PersistBeforeAck |
| `commit-old-term` | leader commits old-term entries by counting replicas (Figure 8) | 100,000 | 185 | 227 | CurrentTermCommit, Linearizability |
| `get-from-memory` | leader serves Get from memory, not the log | 100,000 | 59 | 148 | Linearizability |
| `no-leader-noop` | new leader skips the current-term no-op | 100,000 | 12 | 5,287 | Linearizability |
| `drop-idempotency` | state machine ignores the idempotency table | 100,000 | 9 | 28,061 | Linearizability |

Four of six are rare: detection rates from 0.185% down to 0.009% mean the default 1000-seed CI swarm would usually miss them. The nightly swarm stays at 1000 seeds; the pinned first-detection seeds above are regression tests (`crates/chronos-sim/tests/mutants.rs`). Measured throughput is about 2 ms per seed (20,000 seeds in 40.6 s).

## Agent harness

[`agent/`](agent/README.md) applies the same method (seed, trace digest, delta debugging) to tool-using LLM agents: seeded tool faults, invariants, shrinking, record/replay (including cut-point replay) of model calls, OpenTelemetry-style span export, and a bubblewrap command sandbox with a [threat model](docs/threat-model.md). Try `python -m chronos_agent demo` from `agent/`. Scripted agents only; no live-model run has been executed.

## Limitations

- Swarm workloads are small: at most 40 client ops per run (8 to 32 typical), two keys, a 5 s virtual horizon in brutal runs. The linearizability checker caps at 48 ops.
- Coverage counts which fault types fired, not protocol states reached.
- No real (unplanted) protocol bug has been found; the mutant table measures the oracles against known bugs only.
- Static membership, no snapshots, no ReadIndex; `chronos-node` is a single-process WAL demo.

---

## Build and test

```bash
git clone https://github.com/sahilahmed21/Chronos.git
cd Chronos

cargo build --workspace --release
cargo test --workspace --release
cargo clippy -p chronos-protocol -p chronos-sim --all-targets -- -D warnings
```

On Windows without a local MSVC linker, use Docker:

```bash
docker run --rm -v c:/projects/Chronos:/work -w /work -e RUST_TEST_THREADS=1 \
  rust:1-bookworm cargo test --workspace --release
```

---

## Demo

### Deterministic seed

```bash
cargo run -p chronos-sim --release -- --seed 7
```

Prints a SHA-256 digest. Running the same seed again yields the **same** digest.

### Swarm batch

```bash
mkdir -p traces
cargo run -p chronos-sim --release -- --seeds 32 --start 1 --out traces --coverage
```

Exit code `0` if every seed passes. On failure, writes `traces/fail-<seed>.trace` and a planned schedule. `--coverage` reports which fault combinations fired.

### Replay and minify

```bash
cargo run -p chronos-sim --release -- --replay traces/fail-<seed>.trace
cargo run -p chronos-sim --release -- --minify --seed <seed> --out traces
```

### Planted Election Safety regression

```bash
cargo test -p chronos-sim --features mutants --test properties planted_skip_vote_persist_fails_election_safety -- --exact
cargo test -p chronos-sim --features mutants --test properties unplanted_solo_campaign_passes_persist_before_send -- --exact
```

The planted hook needs `--features mutants` and is never enabled in swarm or CLI minify.

### Mutant detection

```bash
cargo run -p chronos-sim --release --features mutants -- --mutant commit-old-term --seeds 100000
```

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

## Architecture

1. **`chronos-protocol`** — `step(state, event) → Vec<Effect>`. No clocks, sockets, files, threads, or `HashMap`.
2. **`chronos-sim`** — single-threaded tickless scheduler (heap key `(time, global seq)`), seeded RNG, simulated net/disk, checkers, swarm, minify.
3. **`chronos-node`** — interprets the same effects against a real append-only WAL (`[len][crc32][payload]`).

Disk model: process-visible buffer vs durable watermark; torn writes only past the durable prefix; fsync-lie is modeled but off by default.

---

## Documentation

| Document | Contents |
|----------|----------|
| [docs/00-project-guide.md](docs/00-project-guide.md) | Problem statement, architecture overview, and Raft primer |
| [docs/01-understanding.md](docs/01-understanding.md) | Shared vocabulary — DST, Raft safety properties, product constraints |
| [docs/02-architecture.md](docs/02-architecture.md) | Locked design decisions (D1–D17) — the architectural constitution |
| [docs/03-research.md](docs/03-research.md) | Prior art survey — FoundationDB, TigerBeetle, Dropbox Nucleus, PAR |

---

## License

MIT — see [LICENSE](LICENSE).
