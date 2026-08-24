# P9 — Product quality

**Status:** implemented (tooling + docs + CI + coverage + PLANTED pack); **verified green** blocked on Linux CI / local `link.exe`  
**Serves:** G11  
**Must not:** claim fsync-lie / Byzantine / Jepsen coverage; hide non-goals

**Done when:** README is a standalone portfolio page; CI blocks merges on new invariant failures; coverage of fault combinations is visible.

**Files:** `README.md`, `.github/workflows/ci.yml`, `scripts/`, `docs/bugs/`, coverage output from `chronos-sim`

**Locked defaults (architecture review):** PR `--seeds 32 --start 1`; nightly `1000`; any `CheckFail` (including abort) fails the job; PLANTED pack allowed and labeled; Linux required; Windows CI optional follow-up.

---

## P9.A — README as product

### P9.A.1

- **P9.A.1.a** What it is (correctness engine, not “yet another Raft”).
- **P9.A.1.b** Build/test/run sim/replay seed/minify.
- **P9.A.1.c** Architecture one-pager: step, two interpreters, `(time, seq)`, WAL CRC.
- **P9.A.1.d** Scope: in vs out (D10, membership, snapshots, read-index, Jepsen).
- **P9.A.1.e** Link to `docs/bugs/` and roadmap.

**Exit:** someone with Rust installed can replay the artifact from README alone.

---

## P9.B — CI

### P9.B.1 Gates

- **P9.B.1.a** `cargo test --workspace`
- **P9.B.1.b** `cargo clippy -p chronos-protocol -p chronos-sim -- -D warnings` (disallowed types)
- **P9.B.1.c** Grep: no `HashMap`/`HashSet` in protocol or sim source (except comments/`clippy.toml`)
- **P9.B.1.d** Grep: no `Instant`/`std::fs`/`std::net`/`thread::spawn` in protocol
- **P9.B.1.e** Determinism: seed 42 twice
- **P9.B.1.f** Swarm: N seeds (PR 32, nightly 1000). Fail job on invariant violation
- **P9.B.1.g** Planted votedFor test still fails the hook / still passes production code

### P9.B.2 Platforms

- **P9.B.2.a** Linux CI required. Windows if available (this repo started on Windows). Codec/digest must match.

**Exit:** a PR that adds `HashMap` to `cluster.rs` is red.

---

## P9.C — Coverage

### P9.C.1

- **P9.C.1.a** Counters: partition used, crash used, torn suffix nonempty, fsync Err, drop, dup, buggify flags, n=3 vs n=5.
- **P9.C.1.b** Pairwise: e.g. crash **during** partition. Print top combos after a batch.
- **P9.C.1.c** Do not claim “all combinations.” Claim “these fired in the last batch.”

**Exit:** `chronos-sim --seeds 100 --coverage` prints a table.

---

## P9.D — Polish

### P9.D.1

- **P9.D.1.a** License, rust-version if you pin one.
- **P9.D.1.b** `docs/roadmap/` marked complete/incomplete per phase status lines.
- **P9.D.1.c** Interview cheat-sheet already lives in `docs/00-project-guide.md`; README points to it.

**Exit:** [deliverable.md](deliverable.md) checklist all ticked.

---

## Phase exit checklist

- [x] README standalone (includes PLANTED reproduce via `cargo test`)
- [x] CI workflow present (HashMap gates + swarm); **first green run still required on GitHub**
- [x] Coverage output (`--seeds … --coverage`)
- [x] Bug artifact linked (PLANTED; not a wild PROTOCOL find)
- [x] Non-goals stated

**v1 verified** only after: Linux CI green once, linker-capable `cargo test --workspace`, and `COMMIT` filled after `git init`.
