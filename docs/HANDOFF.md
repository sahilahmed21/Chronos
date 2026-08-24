# Session handoff — Chronos

**Date:** 2026-08-25  
**State:** v1 tooling **shipped** (P1–P9). Seed **281** closed as **CHECKER** (plus HARNESS heartbeat book). Public repo [sahilahmed21/Chronos](https://github.com/sahilahmed21/Chronos). Checker/heartbeat fixes may still be uncommitted locally.

| Pin | SHA |
|-----|-----|
| HEAD | `cd35c46260f8076725a2c2766f1d51c013972de8` |
| Green code / planted `COMMIT` | `2b46ebb0c4216ead35a52352460a372e85ffae98` (do **not** re-pin to HEAD unless asked) |

**Proof:** `ci / verify` green on both SHAs. Docker `cargo test --workspace --release` green on Windows host (`rust:1-bookworm`). Host MSVC `link.exe` still optional.

**G0:** preferred wild PROTOCOL not found. PLANTED pack remains G9. Hunt seed 281 was a false Linearizability fail (`Io` as definite miss). Docker `--seeds 1000 --start 1` is green on the fixed tree. Nightly Actions should match once this tree is on `main`.

**Do not:** re-init git, rewrite v1 history, force-push, put horizon drain back on tests, drop `maybe_commit`→`replicate_all`, enable `skip_vote_persist` in swarm/CLI minify.

---

## Doc precedence

1. [02-architecture.md](02-architecture.md) — constitution (D1–D17). **Wins all conflicts.**
2. [roadmap/](roadmap/README.md) — sequence G0–G11 / P1–P9.
3. [00-project-guide.md](00-project-guide.md) — original guide; architecture wins on conflicts.

---

## Product

Simulation-first Raft KV. Same `step` under `chronos-sim` and `chronos-node`. Artifact = seed + SHA-256 digest + minimized schedule + writeup.

## Local commands (Windows without `link.exe`)

```text
docker run --rm -v c:/projects/Chronos:/work -w /work -e RUST_TEST_THREADS=1 \
  rust:1-bookworm cargo test --workspace --release

docker run --rm -v c:/projects/Chronos:/work -w /work rust:1-bookworm \
  cargo run -p chronos-sim --release -- --seeds 1000 --start 1 --out traces --coverage
```

Forward slashes on the drive mount. Poll CI with `gh run view <id> --json …` — do **not** `gh run watch` (cancels runs).

## Clean commits (no Co-authored-by)

Agent Shell `git commit` injects Cursor trailers. Write subject to `.git/COMMIT_MSG_CLEAN`, then:

```text
python .git/commit_clean.py .git/COMMIT_MSG_CLEAN <paths...>
```

Helpers live under `.git/` (not in the tree). `.git/rewrite_clean_commit.py` rewrites HEAD if a trailer landed. No `--no-verify`, no force-push, no amend unless asked.

## Hard design locks (do not undo)

- `Cluster::drain()` stops when live leader + heap has no non-timer events. Swarm/minify use `drain_horizon()`.
- `maybe_commit` calls `replicate_all` when commit advances.
- Delayed-message scenario: net delay 400ms, election 2.0–2.5s, `wait_leader()`, `max_ns` 30s.
- Planted `skip_vote_persist` is `#[cfg(test)]` only; never swarm/CLI minify.

## Bug packs

- Shipped: [bugs/2026-08-24-planted-skip-vote-persist/](bugs/2026-08-24-planted-skip-vote-persist/) — CLASS=PLANTED.
- Hunt: [bugs/HUNT-2026-08-24.md](bugs/HUNT-2026-08-24.md) — **CLOSED** CLASS=CHECKER; no PROTOCOL pack.

## Remaining (honest)

| Item | Status |
|------|--------|
| v1 crates + CI + PLANTED + README | Done |
| Seed 281 Linearizability hunt | Closed CHECKER; 1000-seed Docker swarm green |
| Wild PROTOCOL pack + CLI `schedule.min` | Not found; PLANTED remains G9 |
| GitHub nightly 1000 green | Expected once checker/heartbeat commits are on `main` |
| Post-v1 (TCP 3-node, membership, …) | Out of scope |

Roadmap status: [roadmap/README.md](roadmap/README.md). Deliverable checklist: [roadmap/deliverable.md](roadmap/deliverable.md).
