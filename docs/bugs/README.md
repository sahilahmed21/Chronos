# Bug packs (P7)

A pack is a directory, not a single markdown file. Layout matches [deliverable.md](../roadmap/deliverable.md) section 4. P7 writes `schedule.planned`; P8 adds `schedule.min`.

```text
docs/bugs/YYYY-MM-DD-<short-name>/
  README.md           invariant, nodes/terms/indices, why a unit test misses it
  SEED                u64
  COMMIT              git rev-parse HEAD of the reproducing tree
  trace.sha256        digest hex of the failing run
  fail.trace          copy of fail-<seed>.trace
  CLASS               PROTOCOL | SIM | CHECKER | HARNESS | LIMITATION | PLANTED
  schedule.planned    human dump of swarm_plan(seed) extras (not minimized)
  schedule.min        P8 minimized atoms (windows, clients, drop/dup tokens)
  FIX.md              patch summary, or limitation citation
```

Copy [_template/](_template/) and fill it. Do not ship `_template` as a finding.

## CLASS

| Value | Meaning | Pack? |
|-------|---------|-------|
| PROTOCOL | `step` violates a specified invariant | yes — the portfolio artifact |
| SIM | disk/net/sched/trace lied | no — fix sim, re-run the seed |
| CHECKER | oracle false positive | no — fix checker, re-run |
| HARNESS | `CheckerCapacity` / `UnmatchedReply` / planner abort | no — fix harness |
| LIMITATION | known paper-Raft / D10 class | yes — labeled near-miss |
| PLANTED | `skip_vote_persist` (or similar) with the hook **off** in swarm | yes — only if the honest swarm found nothing |

Triage order: HARNESS, CHECKER, SIM first. Never retune election/drop to hide a fail. Never enable fsync-lie or `skip_vote_persist` in swarm to manufacture a find.

## Reproduce

Clean tree. Same commit as `COMMIT`.

```text
chronos-sim --seed <SEED>
chronos-sim --replay fail.trace
chronos-sim --minify --seed <SEED>
```

`--replay` re-runs `run_seed` from the header seed and **verifies** digest, check name, config hex, and extras hex.

Stdout is the verdict line, then the process exits. It never prints hunt `FAIL`/`ok` on `--replay`.

- `CLEAN` / `DID_NOT_REPRODUCE` exit 0 — live run has no check (`DID_NOT_REPRODUCE` means the file named a fail)
- `REPRODUCED` exit 1 — same digest, check, config, extras
- `MISMATCH …` exit 2 — digest, check, config, extras, or not a fail file (including CRLF-normalized header)

`scripts/repro.ps1 <seed>` / `scripts/repro.sh <seed>` runs the seed twice and compares hunt status lines (`ok`/`FAIL`/`ABORT`). That is G8 for a seed, not `--replay`.

`COMMIT` is unsatisfiable until this tree is a git repository. Do not invent a hash.

## Hunt (blocked on linker here)

```text
scripts/fuzz.ps1 100
scripts/fuzz.sh 100
```

Writes `traces/fail-<seed>.trace` and `traces/fail-<seed>.planned` on CheckFail. Start at 100 seeds; 1000 if that batch is clean. 10k is a stretch goal.

`chronos-sim --minify --seed S --out traces` writes `traces/fail-S.min` (schedule.min text). Success is the same `CheckName`, not D13 digest equality. Copy that file into the pack as `schedule.min`. Do not enable `skip_vote_persist` on this path.

If the swarm is honestly clean, ship a **PLANTED** pack from `crates/chronos-sim/tests/properties.rs` (`planted_skip_vote_persist_fails_election_safety`). The README must say it was not a wild find.

Shipped example: [2026-08-24-planted-skip-vote-persist/](2026-08-24-planted-skip-vote-persist/).
