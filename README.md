# Chronos

**Deterministic simulation testing for a Raft key-value store, with measured oracle detection power.** The same Raft state machine runs under a deterministic simulator — where every source of nondeterminism (time, network, disk, entropy) is controlled — and under a production-style node that uses the real OS. Failures produce a **seed**, a **SHA-256 trace digest**, and a **minimized fault schedule** that can be replayed byte-for-byte. Chronos also measures how good its own checkers are: it plants known protocol bugs and counts how many seeds it takes to catch each.

[![CI](https://github.com/sahilahmed21/Chronos/actions/workflows/ci.yml/badge.svg)](https://github.com/sahilahmed21/Chronos/actions/workflows/ci.yml)

## Oracle detection power

Six deliberate protocol bugs ("mutants") are compiled in only with the `mutants` feature ([ADR-0002](docs/adr/0002-feature-gated-mutants.md)). Each was run through the swarm with the bug active on every node (`chronos-sim --mutant <name> --seeds N`, release build, WSL2 Ubuntu, 2026-11-10, `--start 0`). The unmutated code stays clean over the same seeds (the 1000-seed CI swarm, plus the test `clean_swarm_stays_clean_with_the_feature_compiled_in`).

| Mutant | Bug | Seeds run | Detected | First seed | Check that fired |
|---|---|---|---|---|---|
| `skip-vote-persist` | vote sent/granted before term+votedFor fsync | 2,000 | 2,000 | 0 | PersistBeforeSend |
| `ack-before-fsync` | follower acks AppendEntries before fsync | 2,000 | 1,580 | 0 | PersistBeforeAck |
| `commit-old-term` | leader commits old-term entries by counting replicas (Figure 8) | 100,000 | 185 | 227 | CurrentTermCommit, Linearizability |
| `get-from-memory` | leader serves Get from memory, not the log | 100,000 | 59 | 148 | Linearizability |
| `no-leader-noop` | new leader skips the current-term no-op | 100,000 | 12 | 5,287 | Linearizability |
| `drop-idempotency` | state machine ignores the idempotency table | 100,000 | 9 | 28,061 | Linearizability |

**Why the nightly runs 100,000 rotating seeds.** The nightly used to run seeds 1..1000 (`--start 1`), the same seeds every night. Run against each mutant, that range detects `skip-vote-persist` on 1000/1000 seeds, `ack-before-fsync` on 789, `commit-old-term` on 4 (first seed 227), `get-from-memory` on exactly 1 (seed 148), and `no-leader-noop` and `drop-idempotency` on none. So it caught four of six, two of them only by luck of the range, missed two, and could never find anything new. On a fresh 1,000 seeds, the chance of at least one hit is about 84% for `commit-old-term`, 45% for `get-from-memory`, 11% for `no-leader-noop` and 9% for `drop-idempotency` (from the rates above). The nightly now runs 100,000 seeds starting at `run_number * 100000`, so each night covers new seeds, and it logs the start seed (job log and step summary) so any failure reproduces. A 100,000-seed run with `--coverage` took 3 min 9 s locally; GitHub runner time is untested. The pinned first-detection seeds are regression tests (`crates/chronos-sim/tests/mutants.rs`). Measured throughput is about 2 ms per seed (20,000 seeds in 40.6 s).

### Does a bigger workload catch the rare four sooner?

No. Detections per 100,000 seeds (first seed in parentheses), one factor changed at a time, defaults otherwise (8 to 32 ops, two keys, 2 s calm / 5 s brutal horizon). Flags: `--ops`, `--keys`, `--horizon-ms` with `--mutant`.

| Workload | commit-old-term | get-from-memory | no-leader-noop | drop-idempotency |
|---|---|---|---|---|
| default | 185 (227) | 59 (148) | 12 (5,287) | 9 (28,061) |
| 20 ops | 168 (227) | 45 (148) | 12 (7,088) | 10 (6,167) |
| 40 ops | 163 (532) | 105 (125) | 11 (2,964) | 5 (9,670) |
| 1 key | 157 (532) | 93 (479) | 7 (5,430) | 5 (5,430) |
| 4 keys | 183 (227) | 40 (148) | 10 (4,801) | 7 (4,801) |
| 1 s horizon | 18 (8,361) | 5 (18,822) | 1 (63,111) | 3 (27,903) |
| 2 s horizon | 57 (3,377) | 32 (1,580) | 6 (36,406) | 6 (36,406) |
| 10 s horizon | 151 (818) | 38 (2,876) | 10 (4,534) | 7 (23,513) |
| 40 ops, 1 key, 2 s | 57 (71) | 90 (218) | 9 (8,830) | 7 (17,068) |

Reading it: counts of 5 to 12 carry Poisson noise of roughly ±2 to ±4, so most cells match the default. Only `get-from-memory` moves clearly (105 at 40 ops, 93 at one key, about 1.6x to 1.8x). A short horizon is clearly worse, and nothing helps `no-leader-noop` or `drop-idempotency`. I did not test why: the likely limit is the fault timing needed (a leader change at the wrong moment), not the amount of client load. The default workload stays. What would help is more seeds: 100,000 seeds takes about 200 s and catches all four.

## Agent harness (scripted agents only)

[`agent/`](agent/README.md) applies the same method (seed, trace digest, delta debugging) to tool-using agents: seeded tool faults, invariants, and shrinking of the fault schedule. **Every agent number comes from scripted Python agents, not a real model.** Real-model evals and record/replay of model calls belong to Atlas, a separate repo. Try `python -m chronos_agent demo` from `agent/`.

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

CI runs **32** seeds (`--start 1`) on push/PR and **100,000** seeds on the nightly schedule, starting at `run_number * 100000` and logged in the job summary. Manual `workflow_dispatch` takes `swarm_seeds` and `swarm_start`. Workflow: [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

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
