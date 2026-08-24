# Chronos

A **simulation-first Raft key-value store**. The product is a correctness engine: the same protocol code runs under a deterministic simulator (we own time, network, disk, entropy) and under a production node (the OS owns those).

If a safety property breaks, the artifact is a **seed**, a **SHA-256 trace digest**, and a **minimized fault/delivery schedule** — not a flake.

## Build

```text
cargo build --workspace
cargo test --workspace
cargo clippy -p chronos-protocol -p chronos-sim --all-targets -- -D warnings
```

**Linux CI** (`.github/workflows/ci.yml`) is the required gate — `ci / verify` is green on `main`.

On Windows without MSVC `link.exe`, use Docker (forward slashes on the mount):

```text
docker run --rm -v c:/projects/Chronos:/work -w /work -e RUST_TEST_THREADS=1 \
  rust:1-bookworm cargo test --workspace --release
```

Determinism source gates (no `HashMap` in protocol/sim; no `std::fs` / `Instant` / … in protocol):

```text
# Linux / macOS
bash scripts/check-determinism-gates.sh
# Windows
powershell -NoProfile -File scripts/check-determinism-gates.ps1
```

## Run the simulator

```text
# One seed → digest + observed fault flags
cargo run -p chronos-sim -- --seed 7

# Batch (PR default: 32 seeds from 1). Exit 1 if any CheckFail (including abort).
cargo run -p chronos-sim -- --seeds 32 --start 1 --out traces --coverage

# Replay a fail file (verifies digest, check, config, extras)
cargo run -p chronos-sim -- --replay traces/fail-<seed>.trace

# Minimize a failing seed's schedule (same CheckName oracle)
cargo run -p chronos-sim -- --minify --seed <seed> --out traces
```

`--coverage` prints **profile** knobs (`n`, brutal, buggify) separately from **observed** fault fires (crash, partition, drop, …) and top observed pairs. It does **not** claim all combinations were explored.

## Architecture (one pager)

```text
                    chronos-sim              chronos-node
                    (scheduler, faults)      (files / OS)
                              \                /
                               v              v
                         chronos-protocol
                    fn step(node, event) -> Vec<Effect>
```

- **Tickless** `u64` nanoseconds; heap key `(time, global seq)`.
- Raft never blocks on I/O: it emits effects; interpreters complete them later.
- WAL: little-endian `[len][crc32(len||payload)][payload]` (`Meta` + `Entry`).
- Trace oracle: SHA-256 of encoded records (no host fields). Same seed ⇒ same digest on Windows and Linux.
- Spec: [docs/02-architecture.md](docs/02-architecture.md) (D1–D17). Wins all conflicts.

## Scope

**In:** deterministic DST harness; Raft election/replication; Get via log; white-box safety + linearizability; swarm fuzz; schedule minify; planted `votedFor` regressions.

**Out (v1 non-goals):** Byzantine nodes; dynamic membership; snapshots; lease/read-index; multi-DC clock drift; Jepsen; fsync-lie default-on; Antithesis/Hermit of an unmodified binary.

## Bug packs

See [docs/bugs/](docs/bugs/README.md). Current pack:

- [docs/bugs/2026-08-24-planted-skip-vote-persist/](docs/bugs/2026-08-24-planted-skip-vote-persist/) — **PLANTED** (harness proof; not a wild swarm find).

Reproduce the PLANTED pack (not a swarm `--replay` file):

```text
cargo test -p chronos-sim --test properties planted_skip_vote_persist_fails_election_safety -- --exact
cargo test -p chronos-sim --test properties unplanted_solo_campaign_passes_persist_before_send -- --exact
```

Wild finds (when present) use `chronos-sim --seed` / `--replay` / `--minify` as in the pack README.

**Hunt 281 (CLOSED, CLASS=CHECKER):** [docs/bugs/HUNT-2026-08-24.md](docs/bugs/HUNT-2026-08-24.md) — false Linearizability (`Io` treated as a definite miss). `--seed 281` is clean; local 1000-seed swarm green. PLANTED pack remains G9 (not a wild PROTOCOL find).
## Docs

| Doc | Role |
|-----|------|
| [docs/02-architecture.md](docs/02-architecture.md) | Constitution |
| [docs/roadmap/](docs/roadmap/README.md) | Goals G0–G11, phases P1–P9 |
| [docs/roadmap/deliverable.md](docs/roadmap/deliverable.md) | v1 done definition |
| [docs/00-project-guide.md](docs/00-project-guide.md) | Interview cheat-sheet + original guide |
| [docs/HANDOFF.md](docs/HANDOFF.md) | Current agent handoff (2026-08-25) |
| [docs/bugs/HUNT-2026-08-24.md](docs/bugs/HUNT-2026-08-24.md) | 1000-seed hunt log (seed 281) |

## License

MIT — see [LICENSE](LICENSE). Rust edition 2021 (`Cargo.toml`).
