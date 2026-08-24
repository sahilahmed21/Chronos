# Final deliverable (v1)

This is the definition of done for the **product**, not for a single phase.

## Must exist

### 1. Runnable system

- `cargo test --workspace` green on Windows and Linux.
- `chronos-sim` can run a seed and print a SHA-256 trace digest.
- `chronos-node` can run a single-node WAL KV at minimum; three processes on localhost is enough for the demo. Same protocol crate.

### 2. Determinism proof

- A CI test: `hash(run(s)) == hash(run(s))` for a fixed seed.
- Digest is SHA-256 of encoded `TraceRecord`s. No paths, no wall clock, no `std::hash::Hasher`.

### 3. Safety proof

- Five Raft safety checks after every event (when Raft exists).
- Persist-before-send/ack checks.
- Client-history linearizability at end of run.
- Planted `votedFor` persistence skip still fails Election Safety (kept as a regression test, behind a feature or a copy of the bug in `tests/planted/`).

### 4. The artifact (P7 + P8)

A directory, e.g. `docs/bugs/YYYY-MM-DD-<short-name>/`:

| File | Contents |
|------|----------|
| `README.md` | Invariant, why it is real, why a unit test would not have caught it |
| `SEED` | Integer seed |
| `COMMIT` | git commit that reproduces |
| `trace.sha256` | Digest of the full failing run |
| `schedule.min` | Minimized fault/delivery schedule |
| `FIX.md` | Patch summary, or “near-miss, not a product bug” with protocol-limitation citation |

Replay: `chronos-sim --seed <SEED>` on that commit → same digest and same check name.

Fallback allowed by the original guide: if swarm finds nothing, ship a **documented near-miss** using a production-class planted fault (persist-after-send, old-term commit, Get-from-leader-memory). Label it a near-miss. Do not pretend it was discovered in the wild.

### 5. Product surface (P9)

- README: install, run sim, replay, what we do **not** claim (fsync-lie, Byzantine, membership, snapshots, Jepsen).
- CI workflow: clippy disallowed-types, protocol/sim HashMap grep, determinism test, N random seeds.
- Coverage log: which fault knobs (and pairs) fired across the seed batch.

## Explicitly not required for v1

- Dynamic membership, snapshots, read-index, performance, Antithesis, Jepsen, fsync-lie default-on.

## Checklist (v1)

- [x] Runnable crates + `chronos-sim` CLI (`--seed` / `--seeds` / `--replay` / `--minify` / `--coverage`)
- [x] Determinism gates + CI workflow (Linux)
- [x] Safety checkers + planted votedFor regressions
- [x] Bug pack linked (`docs/bugs/2026-08-24-planted-skip-vote-persist/`, CLASS=PLANTED)
- [x] README product surface + non-goals
- [x] `cargo test --workspace` green on a machine with a linker (Linux CI; local Windows may use Docker `rust:1-bookworm` when `link.exe` is missing)
- [x] `COMMIT` file filled (`docs/bugs/2026-08-24-planted-skip-vote-persist/COMMIT` = green code SHA `2b46ebb…`, not necessarily HEAD)

## Stronger claims (honest)

Playbook: [../bugs/CLOSE-281.md](../bugs/CLOSE-281.md) **CLOSED** CLASS=CHECKER. PLANTED remains G9.

- [x] Seed-281 triage: HARNESS heartbeat book + CHECKER `Io`-as-unknown ([../bugs/HUNT-2026-08-24.md](../bugs/HUNT-2026-08-24.md))
- [x] Local Docker 1000-seed swarm green (2026-08-25)
- [x] GitHub 1000-seed swarm green — [Actions run 32767856965](https://github.com/sahilahmed21/Chronos/actions/runs/32767856965) (`workflow_dispatch` `swarm_seeds=1000` on `93880c5`; `# coverage runs=1000`)
- Wild PROTOCOL pack with CLI `schedule.min` was **not found**; not a remaining v1 blocker.
## Interview bar

You can:

1. Replay a seed in front of someone.
2. Point at a 12-line minimized schedule and explain the invariant.
3. Say what the simulator does not cover, and why that is deliberate.
