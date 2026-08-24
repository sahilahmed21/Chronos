# P3 — Raft

**Status:** implemented  
**Serves:** G6  
**Must not:** snapshots, membership, read-index, fsync-lie on, fuzzing, “optimize later” threads

**Done when:** 3 nodes, **no faults**, one leader, logs identical for committed prefix, KV Get/Put linearizable on the happy path (history may be checked loosely; full Porcupine is P5).

**Files:** `chronos-protocol/src/raft/*`, `kv/machine.rs`, `chronos-sim/src/cluster.rs`

---

## P3.A — Messages and log

### P3.A.1 RPC types

- **P3.A.1.a** `RequestVote` / `RequestVoteResp` / `AppendEntries` / `AppendEntriesResp` in `Message`. Terms, lastLogIndex/term, prevLogIndex/term, entries, leaderCommit, success, match index.
- **P3.A.1.b** Codec for messages (same little-endian family as WAL).

### P3.A.2 Log

- **P3.A.2.a** `log.rs`: dummy entry at index 0. Append, truncate after conflict, last index/term.
- **P3.A.2.b** Entries carry `term` + payload (`client, request, cmd` or `NoOp`).

**Exit:** log unit tests: append, truncate, match prev.

---

## P3.B — Persist cookies and Recover

### P3.B.1

- **P3.B.1.a** `PersistCookie { term, voted_for, last_index, last_term }`.
- **P3.B.1.b** WAL: persist Meta before vote send; persist Entry+Meta as needed. Always `Append` complete records then `Fsync`. Never in-place.
- **P3.B.1.c** `Recover`: CRC scan, last Meta wins for term/votedFor, rebuild log from Entry records, bump incarnation, clear nextIndex/matchIndex, follower role, `commitIndex = 0` then learn via AE (or last durable if you choose a conservative 0).

**Exit:** crash+recover test on one node restores term/vote/log prefix.

---

## P3.C — Election (persist-before-send)

### P3.C.1 Follower timeout

- **P3.C.1.a** `TimerFired(Election)`: increment term, vote for self **in memory**, emit `IoSubmit(Append Meta)`, `IoSubmit(Fsync)`, `ArmTimer(Election)`. **No Send** in this batch.
- **P3.C.1.b** `IoComplete(Fsync, Ok)` for that cookie: then `Send RequestVote` to all peers. If Err: stay follower/candidate without sending.

### P3.C.2 Granting a vote

- **P3.C.2.a** On `RequestVote`: if grant is allowed, persist Meta first, **Send grant only after** that fsync Ok.
- **P3.C.2.b** Reject (term stale, already voted, log not up-to-date) may Send without persist if state did not change. If term updates, persist first.

### P3.C.3 Become leader

- **P3.C.3.a** Majority grants (including self, and self’s vote is durable) → leader. Init nextIndex/matchIndex.
- **P3.C.3.b** Append **no-op** (current term) and replicate (G6.4). Do not commit old-term entries by counting replicas.

**Exit:** 3-node sim, no faults, eventually exactly one leader per current term; others followers.

---

## P3.D — Replication (persist-before-ack)

### P3.D.1 Leader

- **P3.D.1.a** Heartbeat = empty AE on Heartbeat timer. `ArmTimer(Heartbeat)` interpreted by harness.
- **P3.D.1.b** Client Put/Get: append Entry (in memory + `IoSubmit`), `Send` AE to followers **in the same batch is legal**. Do **not** bump `matchIndex[self]` until own fsync Ok.
- **P3.D.1.c** Commit: majority matchIndex **and** `entry.term == currentTerm`. Then apply inside `step`, `Reply`.

### P3.D.2 Follower

- **P3.D.2.a** Reject AE if prev mismatch. Truncate conflict then append.
- **P3.D.2.b** Successful AE: persist entries, Send success **only after** fsync Ok (persist-before-ack).
- **P3.D.2.c** Advance commitIndex from leaderCommit (capped at last durable matching prefix). Apply inside `step`.

### P3.D.3 Figure 8 guard

- **P3.D.3.a** Explicit test fixture later in P5; in P3 at least: code will not commit previous-term entries by replica count.
- **P3.D.3.b** Stale leader (higher term seen) steps down.

**Exit:** Put on leader, all 3 apply the same command at that index; Get via log returns it.

---

## P3.E — KV

### P3.E.1

- **P3.E.1.a** Idempotency table updated only in apply. Duplicate `(client, request)` returns stored resp.
- **P3.E.1.b** Simulated client retries with same ids (even in P3 happy path, one retry test).

**Exit:** double Put with same request id mutates once.

---

## Phase exit checklist

- [ ] One leader, no faults
- [ ] Logs match on committed entries
- [ ] Get/Put work
- [ ] Vote Send never in the same batch as its persist Fsync
- [ ] AE success Send never before fsync Ok
- [ ] No snapshots, no membership
