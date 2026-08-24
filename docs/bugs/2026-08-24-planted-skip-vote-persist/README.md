# Finding: ElectionSafety (planted skip vote persist)

- Seed: see `SEED` (Cluster RNG seed for the planted test — **not** a swarm hunt seed)
- Commit: see `COMMIT`
- Trace SHA-256: see `trace.sha256`
- Class: see `CLASS`
- Reproduce: `cargo test -p chronos-sim --test properties planted_skip_vote_persist_fails_election_safety -- --exact`
- Unplanted control: `cargo test -p chronos-sim --test properties unplanted_solo_campaign_passes_persist_before_send -- --exact`

## What broke

**Check:** `ElectionSafety` — two leaders in the same term.

**How:** `Cluster::set_skip_vote_persist(NodeId(0), true)` skips Meta fsync before sending/granting votes. After a crash/recover + partition schedule, node 0 can grant in a term without durable `votedFor`, then another candidate wins the same term.

**Nodes:** 3-node static cluster. Scripted partitions and timers (see `schedule.min`).

## Schedule

See `schedule.planned` and `schedule.min`. This pack is **not** produced by `chronos-sim --minify`; the schedule is the planted integration test's inject sequence.

## Why a unit test of `handle_request_vote` would miss it

The bug needs crash + recover + partition heal + timer injection across multiple nodes so two leaders of one term become observable. A single-function unit test that always persists Meta will not trip Election Safety.

## Root cause

Persist-before-send / persist-before-grant violated by the planted hook. Production code keeps the hook **off**. Swarm never enables `skip_vote_persist`.

## Fix or limitation

See `FIX.md`. Harness proof only — not a wild PROTOCOL find.
