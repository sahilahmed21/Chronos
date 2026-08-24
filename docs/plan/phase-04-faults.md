# Phase 4 — Fault injection

**Serves:** G3, G4 (exercised), Level-2 “predict then observe”  
**Do not:** turn fsync-lie **on** in default runs (D10). Do not call a liveness check under a permanent partition.  
**Files:** `chronos-sim/src/{net,disk,cluster,fuzz}` (scripted schedules, not swarm yet)  
**Exit gate:** Named scenarios are **predicted in comments/docs**, then observed. Persist-before-send crash scenario is one of them.

## Subphase 4.1 — World events

### Step 4.1.1 — Types (harness, not `Event` enum)
- [ ] `Crash { node }`
- [ ] `Restart { node }` (crash + later Recover)
- [ ] `Partition { off: pairs or set vs set }`
- [ ] `Heal`
- [ ] Optional: `Drop { msg_id }`, `Duplicate { msg_id }`

### Step 4.1.2 — Crash semantics
- [ ] Discard volatile node state
- [ ] Drop **in-flight disk I/O** for that node (no late `IoComplete`)
- [ ] Messages already in the network **may still deliver** (Raft terms handle stale)
- [ ] Torn suffix past `durable_len` from in-flight Appends (knob on)
- [ ] Then `Recover` with incarnation bump; stale-incarnation completions ignored

### Step 4.1.3 — Verify
- [ ] Unit on disk: crash mid-append → CRC truncate, no durable lie
- [ ] Unit: in-flight fsync never completes after crash

## Subphase 4.2 — Network faults

### Step 4.2.1
- [ ] Delay from RNG (already in Phase 2) — now used on purpose
- [ ] Loss probability per send
- [ ] Duplicate probability
- [ ] Symmetric partition matrix; asymmetric as a **flag**, not a second type

### Step 4.2.2 — Verify
- [ ] Fully partitioned node receives nothing from the other set
- [ ] Heal restores delivery
- [ ] Same seed → same drops (recorded on trace)

## Subphase 4.3 — Disk faults (honest)

### Step 4.3.1
- [ ] Delay I/O completions (on)
- [ ] Fsync returns `Err` (on) — node must not Send vote/ack
- [ ] Torn suffix (on)
- [ ] Fsync-lie (`Ok` without advancing `durable_len`): **implemented, default off**

### Step 4.3.2 — Verify
- [ ] Fsync Err: durable cookie unchanged; no RequestVote send
- [ ] Fsync-lie off: a test that *enables* it is allowed, but default swarm later must not

## Subphase 4.4 — Scripted scenarios (predict → run)

Write the prediction **before** looking at the trace.

### Step 4.4.1 — Partition the leader
- [ ] **Predict:** minority leader cannot commit; majority elects new leader with higher term; on heal, old leader steps down; uncommitted minority entries may be overwritten
- [ ] **Observe:** matches prediction; no two leaders **same term**

### Step 4.4.2 — Crash after persist-before-send split
- [ ] **Predict:** if we crash after RequestVote is on the wire but before fsync, restarting node may vote twice in that term → Election Safety dies. Production code must not emit that Send. If we **force** a planted send-before-persist, checker (Phase 5) will catch it. For Phase 4, crash **between** fsync submit and complete: node restarts, durable vote missing, must not have sent. If it sent, that is a G5 bug — stop and fix.
- [ ] **Observe:** no RequestVote on wire unless `durable` matches

### Step 4.4.3 — Torn tail
- [ ] **Predict:** last in-flight Entry/Meta may vanish; earlier fsynced records remain
- [ ] **Observe:** CRC recover; cluster can still elect after restart

### Step 4.4.4 — Stale leader heals
- [ ] **Predict:** higher term RPC → step down; log truncated/overwritten per matching
- [ ] **Observe:** committed entries remain (Leader Completeness — full check in Phase 5)

## Subphase 4.5 — Record the schedule

### Step 4.5.1
- [ ] Trace includes partitions, crashes, drops, completion times — enough to replay **without** re-drawing RNG after a deletion (needed in Phase 8)
- [ ] At minimum: record every world event with `(time, seq, kind)`

### Step 4.5.2 — Verify (exit gate)
- [ ] Each scenario 4.4.x has a test or a `--scenario` binary flag
- [ ] Predictions are written in this file or `docs/scenarios/` (if you add that folder, keep it short)
- [ ] Default config does not enable fsync-lie

## Stop

Phase 5 checker will make these scenarios machine-checked. If a scenario already violates safety, fix Raft **now**.
