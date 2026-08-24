# F4 — Rust for Chronos (you don’t need to know Rust yet)

**Goal:** After this doc you can answer Rust questions that naturally arise around Chronos — ownership, enums, `Result`/`Option`, crates, `BTreeMap`, Clippy gates, codecs, testing — without claiming to be a Rust expert on unrelated topics (Tokio internals, GATs, `unsafe`, proc macros).

**Audience assumption:** you may have never written Rust. You *have* studied Chronos architecture (F1–F3). Every Rust concept below is taught **only as it appears in this repo**.

**Honest positioning (say early if asked about Rust depth):**

> I’m strongest on systems design and correctness engineering. Rust details I learned on Chronos — ownership around `step`, deterministic collections, effect enums, little-endian codecs. I’m not claiming deep expertise in async Rust or `unsafe`. Happy to walk through Chronos code patterns.

Memorize that script. Interviewers respect scoped honesty more than fake fluency.

---

## 0. What Chronos actually is in Rust terms

| Fact | Where |
|------|--------|
| Workspace of **three crates** | root `Cargo.toml` `members` |
| **Edition 2021** | `[workspace.package] edition = "2021"` |
| **`unsafe_code = "forbid"`** | `[workspace.lints.rust]` |
| Clippy **`disallowed_types` / `disallowed_methods` = deny** | workspace + per-crate `clippy.toml` |
| Protocol: sync `step(&mut Node, Event) -> Vec<Effect>` | `crates/chronos-protocol/src/raft/step.rs` |
| **No async / Tokio** in the protocol | Locked decision **D2** |
| Sim is **single-threaded** discrete-event | `chronos-sim` cluster heap |
| Collections: `Vec`, `BTreeMap`, `BTreeSet` — **not** `HashMap` | Locked decision **D6** |
| Codec: hand-rolled **little-endian**, **no serde** | **D14**, `codec.rs` / `wal/record.rs` |
| `IoId { incarnation, local }` for stale completions | `types.rs`, `step.rs` recover/io_complete |

If an interviewer asks “why Rust?”, answer with Chronos reasons, not language marketing:

> Strong types for a safety-critical state machine, exhaustive enums for the world↔node vocabulary, and a borrow checker that pushes clear ownership between cluster, nodes, and effects — which is exactly what a deterministic simulator wants.

---

## 1. Mental model (5 minutes)

Rust is C-like control flow with **compile-time ownership**:

1. Every value has one **owner**.
2. You can **borrow** immutably (`&T`) many times, **or** mutably (`&mut T`) once — not both at once.
3. When the owner goes out of scope, the value is **dropped** (destructor runs).
4. There is **no GC** and (in Chronos protocol) almost no shared mutable state.

Chronos uses this for:

- `&mut Node` in `step` — exclusive mutation of one replica.
- Cluster owns `Vec`-like node storage; `step` never takes nodes by value.
- Sim is **one OS thread** — logical concurrency (N Raft nodes) without OS data races.
- Effects are **owned** (`Vec<Effect>`) so the harness can queue/interpret them without dangling refs.

**Interview line:** “Concurrency in Chronos is *logical* nodes on one thread, not `Arc<Mutex<Node>>`.”

---

## 2. Crates, workspace, edition, forbid unsafe

### Workspace (root `Cargo.toml`)

```toml
[workspace]
resolver = "2"
members = [
    "crates/chronos-protocol",
    "crates/chronos-sim",
    "crates/chronos-node",
]

[workspace.package]
edition = "2021"
# ...

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
disallowed_types = "deny"
disallowed_methods = "deny"
```

| Term | Meaning in Chronos |
|------|-------------------|
| **Workspace** | One repo, multiple crates sharing edition/lints |
| **Crate** | Compilation + dependency boundary |
| **Module** (`mod raft;`) | Namespace *inside* a crate |
| **Edition 2021** | Language surface Chronos compiles against |
| **`unsafe_code = "forbid"`** | No `unsafe` blocks/functions in workspace code you discuss |

### Why three crates (D5)

| Crate | Role | Allowed to touch |
|-------|------|------------------|
| `chronos-protocol` | Pure Raft/KV `step` | No `fs`, `net`, `Instant`, `HashMap` |
| `chronos-sim` | Deterministic world, checkers, fuzz | May simulate disk/net; still **no HashMap** |
| `chronos-node` | Production interpreter (single-node WAL demo) | Real file I/O OK; must not feed OS entropy into protocol state |

**Interview line:** “Cargo dependency edges are architecture. Sim can’t quietly poison the protocol with `std::net` ‘just in a test’ if they’re separate crates.”

Dependency direction (mental picture):

```text
chronos-sim  ──depends──►  chronos-protocol
chronos-node ──depends──►  chronos-protocol
# protocol does NOT depend on sim or node
```

---

## 3. How a Chronos function looks

Real shape (simplified from `raft/step.rs`):

```rust
pub fn step(node: &mut Node, event: Event) -> Vec<Effect> {
    match event {
        Event::TimerFired { timer } if timer == TIMER_ELECTION => {
            election::on_election_timeout(node)
        }
        Event::MessageReceived { from, msg } => {
            // match on Message::RequestVote / AppendEntries / ...
        }
        Event::IoComplete { id, result } => io_complete(node, id, result),
        Event::Recover { durable } => recover(node, durable),
        Event::ClientRequest { req } => replication::client_request(node, req),
        // ...
    }
}
```

| Syntax | Meaning |
|--------|---------|
| `pub fn` | Visible outside the module/crate (re-exported) |
| `node: &mut Node` | Exclusive borrow for the duration of the call |
| `Event` | Enum — closed set of world→node inputs |
| `-> Vec<Effect>` | Owned growable list of node→world requests |
| `match` | Exhaustive pattern match (compiler errors if a variant is missing) |
| `if timer == …` | Match guard — refine an arm without a new enum variant |

**Interview line:** “`step` takes exclusive access to one node, returns effects; the harness applies effects. No async. No `now`. No RNG inside protocol.”

---

## 4. Enums = tagged unions (Event / Effect / Message)

Chronos’s entire “API between worlds” is enums.

### Event (world → node)

From `event.rs` (conceptually):

```rust
enum Event {
    TimerFired { timer: TimerId },
    MessageReceived { from: NodeId, msg: Message },
    IoComplete { id: IoId, result: Result<(), IoError> },
    Recover { durable: Vec<u8> },
    ClientRequest { req: ClientReq },
}
```

### Effect (node → world)

From `effect.rs`:

```rust
enum Effect {
    Send { to: NodeId, msg: Message },
    IoSubmit { id: IoId, op: IoOp },
    ArmTimer { id: TimerId, kind: TimerKind },
    CancelTimer { id: TimerId },
    Reply { to: ClientId, request: RequestId, resp: ClientResp },
}
```

### Message (Raft RPC payload)

```rust
enum Message {
    Ping,
    RequestVote { term: Term, last_log_index: Index, last_log_term: Term },
    RequestVoteResp { term: Term, granted: bool },
    AppendEntries { /* ... */ entries: Vec<LogEntry>, leader_commit: Index },
    AppendEntriesResp { term: Term, success: bool, match_index: Index },
}
```

**Why Chronos loves enums:**

- The vocabulary is **closed** — adding a variant forces every `match` to update (or use `_` deliberately).
- Replaces C-style “type tag + void*” and Go-style interface soup for the step boundary.
- Effects make I/O **data**, not calls — so the sim can delay/crash/reorder.

**Exhaustiveness interview question:** “What happens if you add `Effect::Apply`?”

> The compiler breaks every match until we handle it — which is why Chronos **rejected** optional Apply (D12): apply stays inside `step`.

---

## 5. `Result` and `Option` (no null, no exceptions)

```rust
Result<T, E>  // Ok(T) or Err(E)
Option<T>     // Some(T) or None
```

Chronos examples:

| Location | Type | Meaning |
|----------|------|---------|
| `IoComplete.result` | `Result<(), IoError>` | Fsync/IO ok vs `FsyncFailed` / `IoFailed` |
| `voted_for` | `Option<NodeId>` | None = haven’t voted this term |
| `Message::encode` | `Option<Vec<u8>>` | None on overflow / encode failure |
| `ClientResp` | enum `Ok` / `Err(ClientError)` | Client-visible errors as **values** |
| Checkers | `Result` / `CheckFail` | Property failure names |

`?` operator: early-return `Err` / `None`. Used more in codecs, CLI, and helpers than deep inside the Raft hot path (which prefers explicit matches and empty `Vec::new()` for “ignore”).

**Protocol style:** don’t panic on a NotLeader client — return `ClientResp::Err(ClientError::NotLeader)`.

---

## 6. Ownership patterns you will be asked about

### 6.1 Why `&mut Node`, not `Node` by value?

Cluster owns nodes. If `step` took `Node` by value, you’d move the node out of the cluster, mutate it, and have to put it back every time. `&mut Node` mutates **in place**.

### 6.2 Who owns the `Vec<Effect>`?

`step` **creates** and **returns** it. Ownership moves to the caller (cluster interpreter). The node does not keep a shared pointer to pending effects.

**Strong answer:** “Effects are owned return values. The harness schedules Send/IoSubmit/timers. That split is how we get crash-between-submit-and-complete.”

### 6.3 Why `Clone` on messages / cookies / entries?

Harness, network buffers, and traces need **owned** bytes and structured copies. Chronos clones deliberately for clear ownership — not because of shared mutable state.

### 6.4 Why no `Arc<Mutex<Node>>`?

Single-threaded sim. Mutexes would reintroduce scheduling nondeterminism and hide races. Logical concurrency ≠ OS threads.

### 6.5 Why `Copy` on `NodeId`, `Term`, `Index`, `IoId`, …?

From `types.rs`: small newtypes over integers derive `Copy`. Passing them doesn’t move a heap allocation. They’re cheap keys for `BTreeMap` / heap ordering.

**Strong answer:** “IDs are `Copy` newtypes so we can’t mix Term with Index, and we don’t pay move/clone for every map key.”

### 6.6 Lifetime of `&msg` in interpret / match

When you write:

```rust
Event::MessageReceived { from, msg } => {
    match msg {
        Message::RequestVote { .. } => election::on_request_vote(node, from, msg),
        // ...
    }
}
```

`msg` was **moved** out of the event (or nested-matched). Helpers take `Message` by value (owned) in Chronos’s style, so you don’t keep a long-lived borrow into the cluster’s event queue.

If asked “what is a lifetime?”:

> A lifetime `'a` is a compile-time region a reference must be valid. Chronos returns owned `Vec<Effect>` and owns message bytes, so the public `step` API rarely needs explicit `'a`.

`codec::read_bytes` *does* show an explicit lifetime (slice borrow into input buffer) — good “I’ve seen it” example without overclaiming.

---

## 7. Collections: why `BTreeMap` / `BTreeSet`, not `HashMap`

```rust
use std::collections::{BTreeMap, BTreeSet};
```

| | `HashMap` / `HashSet` | `BTreeMap` / `BTreeSet` |
|--|----------------------|-------------------------|
| Iteration order | Arbitrary / RNG-seeded in std | Sorted by key — **stable** |
| Chronos protocol | **Banned** | Used |
| Chronos sim | **Banned** | Used |
| Chronos node | Allowed only for OS tables that **never** iterate into traces | Prefer BTree when unsure |

### Locked decision D6 (cite this)

From `docs/02-architecture.md`:

> **D6** — BTreeMap/BTreeSet/Vec in protocol **and** sim. Rejected: deterministic hasher; ban HashMap only in protocol.  
> Why: sim iteration (net, disk, check, trace) is what D13 hashes. Hasher is easy to forget.

**Also cite research:** default `HashMap` is randomly seeded from the OS; iteration order changes across runs and Windows vs Linux CI even with a “perfect” scheduler.

**Trap answer to reject:** “We’ll use HashMap with a fixed hasher.”

> Not enough. Iteration order must be stable for digests and checker walks. Chronos chose BTree everywhere the sim iterates.

Where BTrees show up:

- Node `in_flight: BTreeMap<IoId, …>`
- Idempotency / history structures in sim
- Election safety book: leaders observed per term
- Linearizability state maps
- Fuzz profiles / observed fault counts

`Vec` is fine when order is **explicit append order** (log entries, effect lists, WAL scan).

---

## 8. No async / Tokio in the protocol (D2)

Chronos protocol is a **synchronous** function. The simulator owns a tickless event heap keyed by `(Timestamp, global seq)`.

**Why not Tokio + MadSim/Turmoil under Raft?**

| Concern | Chronos answer |
|---------|----------------|
| Hides the step function | Interviewer can’t audit `step` |
| Task scheduler nondeterminism | Another world to seed/mock |
| Windows / `getrandom` / HashMap leaks | Still bite DST |
| Protocol shouldn’t call I/O | Effects already model I/O |

**Interview line:**

> Async Rust is great for servers. Here the requirement is a deterministic single-threaded discrete-event simulator. We own the heap. Protocol never sees async.

---

## 9. Clippy disallowed types (architecture as lint)

Root comment in `clippy.toml`: bans are **per crate**, because `chronos-node` may use real `File` / sockets.

### `crates/chronos-protocol/clippy.toml`

Bans (among others):

- `HashMap`, `HashSet`
- `Instant`, `SystemTime`
- `std::fs::File`
- `TcpStream`, `UdpSocket`
- `JoinHandle`
- Methods: `thread::spawn`, `thread::sleep`

### `crates/chronos-sim/clippy.toml`

Bans `HashMap` / `HashSet` (sim iteration enters the digest — D6/D13).

Plus CI grep scripts (determinism gates). **D16:** Cargo won’t catch `use std::fs`; Clippy + grep will.

**Interview line:** “We treat lint + CI grep as architecture enforcement, not style.”

---

## 10. `IoId` incarnation pattern (stale completions)

From `types.rs`:

```rust
struct IoId {
    incarnation: u64,
    local: u64,
}
```

Rules:

1. Node assigns `IoId` on `IoSubmit`.
2. On **Recover** (crash restart), node bumps `incarnation` and resets `local`.
3. Harness **drops** in-flight I/O on crash.
4. `io_complete` ignores completions whose `id.incarnation != node.incarnation`.

Real guard in `step.rs`:

```rust
fn io_complete(node: &mut Node, id: IoId, result: Result<(), IoError>) -> Vec<Effect> {
    if id.incarnation != node.incarnation {
        return Vec::new(); // stale — ignore
    }
    let Some(job) = node.in_flight.remove(&id) else {
        return Vec::new();
    };
    // ...
}
```

**Why it matters:** a late `IoComplete(Ok)` from a previous life must not alias a new submit and falsely advance `durable` / unlock persist-before-send.

**Read aloud:** “Incarnation is a generation counter for I/O identity across crash/recover.”

---

## 11. Encode little-endian by hand (no serde) — D14

Locked format:

```text
[len u32 LE][crc32(len || payload) u32 LE][payload]
```

Helpers in `codec.rs`:

```rust
fn put_u32_le(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn put_u64_le(buf: &mut Vec<u8>, v: u64) { /* same idea */ }
```

WAL `encode_record` (conceptual):

1. Encode payload bytes.
2. `len = payload.len()` as u32.
3. CRC over `len_bytes || payload` (CRC covers length — torn `len` isn’t silent).
4. Emit `len`, `crc`, `payload`.

Messages: tag byte + LE fields (`Message::encode` / `decode`).

**Why not serde/JSON/bincode?**

- JSON object key order / float pitfalls.
- Tooling-unstable hashes.
- Torn-write story needs a **byte-level** durable prefix (`durable_len`), not a deserializer that “kinda works.”

**Interview line:** “Codec is part of the durability and cross-platform digest story, not a convenience layer.”

---

## 12. Testing Rust the Chronos way

### Unit tests inside modules

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_id_is_eq_and_ord() { /* ... */ }
}
```

`#[cfg(test)]` — compiled only for `cargo test`, not release library consumers.

### Commands to memorize

```text
cargo test --workspace --release
cargo test -p chronos-protocol
cargo test -p chronos-sim --test properties -- --exact planted_election_safety_fails_when_skip_vote_persist
cargo clippy -p chronos-protocol -p chronos-sim --all-targets -- -D warnings
```

| Flag | Meaning |
|------|---------|
| `-p crate` | Only that package |
| `--release` | Optimized; Chronos CI uses this for hang avoidance |
| `-- --exact` | Run one test by full name |
| `RUST_TEST_THREADS=1` | CI: avoid parallel test interference |

Integration tests live under `crates/chronos-sim/tests/*.rs`.

---

## 13. Newtypes (Chronos IDs)

```rust
struct NodeId(pub u8);
struct Term(pub u64);
struct Index(pub u64);
struct Timestamp(pub u64); // virtual ns — never wall clock
struct TimerId(pub u64);
struct MsgId(pub u64);     // assigned by harness at enqueue, not on Send
```

Prevents mixing term with index at compile time. Common Rust pattern; mention when asked about type safety.

`Timestamp` is virtual nanoseconds from sim origin 0 — protocol has **no clock** (D15): timers are `ArmTimer { kind }`, harness picks duration/`now`.

---

## 14. Traits — what Chronos deliberately avoided

Rust traits ≈ interfaces. Chronos **rejected** sync `Disk::write() -> Result` on the protocol (D3):

- Sync I/O cannot interleave crash with in-flight ops.
- Blanket “no Send with IoSubmit” rejects legal leader AppendEntries.

Interpreters (`SimDisk`, file disk) are called from `cluster.rs` / node main — **composition**, not a DI container of traits inside `step`.

**Line:** “In Rust I’d often use traits for backends; we chose effect enums so the protocol never calls I/O at all.”

---

## 15. Error handling vs panics

| Layer | Style |
|-------|--------|
| Protocol client path | `ClientResp::Err(...)` |
| Codecs | `Option` / `CodecError` |
| Checkers | named check failures |
| CLI | `ExitCode` |
| Tests | `assert!` / `assert_eq!` |

Prefer not panicking in protocol on expected client/Raft conditions.

---

## 16. Mini “read this code aloud” drills

Practice speaking these. Labeled **[PSEUDO]** where shortened; **[REAL SHAPE]** when close to repo.

### Drill A — step dispatch **[REAL SHAPE]**

```rust
pub fn step(node: &mut Node, event: Event) -> Vec<Effect> {
    match event {
        Event::IoComplete { id, result } => io_complete(node, id, result),
        Event::ClientRequest { req } => replication::client_request(node, req),
        // ...
    }
}
```

**Say:** “Exclusive borrow of node. Match is exhaustive on Event. Each arm returns owned effects. Caller interprets.”

### Drill B — stale I/O **[REAL SHAPE]**

```rust
if id.incarnation != node.incarnation {
    return Vec::new();
}
```

**Say:** “After Recover, incarnation bumped. Old completions no-op. Prevents aliasing post-restart submits.”

### Drill C — recover rebuilds node **[REAL SHAPE]**

```rust
let incarnation = node.incarnation.saturating_add(1);
*node = Node::new(id, peers);
node.incarnation = incarnation;
// scan durable WAL prefix into term/voted_for/log
```

**Say:** “Crash drops volatile state. Durable bytes replayed. Incarnation monotonic.”

### Drill D — LE put **[REAL SHAPE]**

```rust
buf.extend_from_slice(&v.to_le_bytes());
```

**Say:** “Explicit little-endian. Same bytes on Windows and Linux for digests and WAL.”

### Drill E — interpreter loop **[PSEUDO]**

```text
while let Some(ev) = heap.pop() {
    now = ev.time;
    let effects = step(&mut nodes[i], ev.into_event());
    for eff in effects {
        match eff {
            Send {..} => enqueue_delivery(...),
            IoSubmit {..} => schedule_disk_complete(...),
            ArmTimer {..} => schedule_timer(...),
            Reply {..} => history.record(...),
            ...
        }
    }
    run_checkers()?;
}
```

**Say:** “Single thread. Seeded delays. Checkers after steps. This is the sim — not Tokio.”

### Drill F — why BTree in history **[PSEUDO]**

```rust
type LinState = BTreeMap<Vec<u8>, Vec<u8>>;
ops: BTreeMap<(ClientId, RequestId), HistoryOp>,
```

**Say:** “Walking ops for linearizability must be deterministic. HashMap iteration would flake digests and searches.”

### Drill G — persist-before-send gate **[PSEUDO]**

```text
on vote decision:
  submit Meta append + fsync effects
  remember pending cookie
  do NOT emit Send(RequestVote) until IoComplete Ok advanced durable
```

**Say:** “Rust doesn’t enforce this — the effect ordering and white-box checker do. Planted hook proves the oracle.”

---

## 17. Common interview Rust Qs → Chronos answers

| Question | Chronos-grounded answer |
|----------|-------------------------|
| What is ownership? | Each value has one owner; `step` returns owned `Vec<Effect>`; cluster owns nodes. |
| Why `&mut` not `&`? | Raft mutates role, log, commit, in_flight. |
| Why not return `&[Effect]`? | Would borrow from node/stack; harness needs owned list to queue asynchronously in sim time. |
| Explain `match`? | Exhaustive tagged dispatch on Event/Message/Effect. |
| `Result` vs exceptions? | Errors as values; IoComplete carries `Result`; clients get `ClientResp::Err`. |
| What is `Option`? | `voted_for: Option<NodeId>`; encode returns `Option` on failure. |
| Why `Copy` on NodeId? | Cheap ID newtype; map keys; no heap. |
| Lifetime of references in step? | Public API avoids long borrows by returning owned effects. |
| Why forbid `unsafe`? | Workspace lint; correctness via types + tests, not raw pointers. |
| Why no HashMap? | **D6** — deterministic iteration for traces/checks. |
| Fixed hasher enough? | **No** — banned in sim too. |
| Why no Tokio? | **D2** — own single-thread scheduler. |
| How enforce bans? | Clippy disallowed_types + CI grep (**D16**). |
| What is a crate? | `chronos-protocol` vs `sim` vs `node` dependency firewall (**D5**). |
| How do you test? | `#[cfg(test)]`, `cargo test -p`, `--exact`, release + single thread in CI. |
| Serde? | No — hand LE codec (**D14**). |
| Async in protocol? | No. |
| Threads in protocol? | Forbidden (spawn/sleep disallowed). |
| How are messages encoded? | Tag + `to_le_bytes` fields; decode checks full consumption. |
| IoId design? | `{incarnation, local}`; stale completes ignored. |
| Vec vs array? | Effects and logs grow; `Vec` owns heap buffer. |
| String vs `[u8]`? | KV keys/values are raw bytes (`Vec<u8>`), not UTF-8 strings. |
| Drop / RAII? | Leaving scope frees Vec buffers; sim doesn’t need manual free. |
| Panic in production path? | Avoid for expected Raft/client cases; prefer Err responses. |
| Traits for Disk? | Rejected for protocol; effects instead (**D3**). |
| Edition? | 2021. |
| Workspace lints inherit? | Yes — forbid unsafe workspace-wide. |

---

## 18. Honest “Rust on my resume” script (30–45s)

> Chronos is written in Rust mainly for strong types and fearless refactoring of a safety-critical state machine. I’m strongest on the systems design — deterministic simulation, Raft safety, checkers. The Rust I know deeply is what’s in this codebase: a synchronous `step(&mut Node, Event) -> Vec<Effect>`, exhaustive enums, `Result`/`Option`, BTree maps for deterministic traces (architecture D6), Clippy bans on HashMap/Instant/fs/net in the protocol, hand-rolled little-endian WAL codecs, and an IoId incarnation scheme so stale disk completions can’t apply after recover. We deliberately don’t put Tokio under Raft. If you ask me about Pin or unsafe internals, I’ll say I haven’t needed them here and I’d learn them the same way I learned this stack — against a concrete correctness goal.

---

## 19. What you can skip unless forced

Pin projections, GATs, `unsafe` transmute, proc macros, advanced async (`Future`, `Pin`), custom allocators, interior mutability puzzles (`RefCell` graphs), lock-free atomics.

**Redirect:** “In Chronos we kept the protocol sync and allocation-honest with Vec/BTreeMap.”

---

## 20. 60-second Rust + Chronos combo answer

> Chronos is a simulation-first Raft KV in a Rust 2021 workspace with `unsafe` forbidden. The protocol is a synchronous function mutating a node via `&mut` and returning an enum of effects — no sockets, no filesystem, no Instant, no HashMap. The sim is one thread with a seeded tickless heap. We ban HashMap in protocol *and* sim so trace digests hash stably (D6). Completions carry IoIds with incarnations so crash/recover can’t double-apply I/O. Codecs are little-endian by hand for WAL and messages — no serde. Clippy disallowed types turn architecture into build failures. That’s the Rust that matters for this project.

---

## 21. Study checklist (evening before)

- [ ] Draw crate graph and say D5/D6/D2 out loud  
- [ ] Read `step` match arms once in the repo  
- [ ] Explain IoId incarnation with a crash timeline  
- [ ] Explain BTree vs Hash without saying “faster lookups”  
- [ ] Run (or recite) `cargo test -p chronos-protocol` and `--exact`  
- [ ] Practice honest Rust-strength script once on video  
- [ ] Do F6 Rust section H questions cold  

**Next:** F5 war stories if you haven’t; then F6 question bank drills.
