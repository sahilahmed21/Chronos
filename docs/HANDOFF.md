# Session handoff — Chronos

**Date:** 2026-08-19  
**State:** P1 Foundation **implemented** (types, WAL, single-node `step` KV, sim stub disk, node file disk) **plus post-review durability fixes**. `cargo check` + `clippy -D warnings` green. **`cargo test` has never linked on this machine** (no MSVC/Windows SDK / `kernel32.lib`).  
**Next action:** get a linker, run `cargo test --workspace`, then **P2 simulator** only if tests pass. **No Raft. No Tokio.**

Give this file to the next chat first. Then open `docs/02-architecture.md` (constitution) and `docs/roadmap/P01-foundation.md` (phase file; its **Status: not started** line is stale). Do not re-research FoundationDB / TigerBeetle / Raft unless a locked decision is being challenged. Do not re-do the P1 architecture review; it is done.

Canvases (Cursor UI only, not the repo): `p1-architecture-review.canvas.tsx`, `p1-core-review.canvas.tsx` under the Cursor project `canvases/` folder. Optional. Source of truth is `docs/` + this file.

---

## What this product is

A **simulation-first Raft KV**. The product is a correctness engine, not “another Raft clone.”

Same protocol crate runs under:

- `chronos-sim` — we own time, network, disk, entropy (DST)
- `chronos-node` — OS owns those (production interpreter)

If a safety property breaks, the artifact is **seed + SHA-256 trace digest + minimized schedule + root-cause writeup**. Not a flake.

Folder name Chronos = virtual time. Repo is **not** a git repository unless someone initialized it after this session. **Do not commit unless the user asks.**

---

## Doc precedence (do not invert)

1. **[docs/02-architecture.md](02-architecture.md)** — constitution. D1–D17. **Wins all conflicts.** Amended 2026-08-18.
2. **[docs/roadmap/](roadmap/README.md)** — sequence only (G0–G11, P1–P9). Does not change architecture.
3. **[docs/00-project-guide.md](00-project-guide.md)** — original spec, **verbatim**. Sync `Clock`/`Network`/`Disk` traits, integer ticks, blanket persist-before-send, default fsync-lie, HashMap-only-in-protocol — **02 wins**.

Also: [01-understanding.md](01-understanding.md), [03-research.md](03-research.md), [roadmap/deliverable.md](roadmap/deliverable.md), [roadmap/goals.md](roadmap/goals.md).

---

## This session (2026-08-18 → 2026-08-19) — what happened

One chat. Do not replay it.

1. Loaded handoff + goals + deliverable + architecture + P01.
2. **Architecture & design review of P1** (no code). Locked fill-ins (see below). User treated those as approved and said implement.
3. **Implemented P1** A→E: types, Event/Effect, WAL CRC/scan, `Node::step` single-node KV, sim delay-0 disk, node `FileDisk`, determinism grep tests/scripts.
4. **Core review** found three **blockers** in the durability/apply coupling (not style). User said fix.
5. **Fixed blockers + related majors** (see “P1 contracts after review”).
6. Verified: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo check --workspace --tests --bins`. Tests still cannot **link**.

---

## P1 contracts after review (do not revert)

These amend how P1 was first coded. Architecture Event list had `Recover` with no payload; **this fill-in stands.**

| Topic | Rule |
|-------|------|
| Recover | `Event::Recover { durable: Vec<u8> }`. Interpreter truncates disk; protocol `scan`s and replays. Crash = drop `Node` + in-flight I/O, pass durable prefix. |
| Keys/values | `Vec<u8>`. |
| Get | Logged `Entry`, same persist path as Put (D7). |
| `encode_record` | `Option<Vec<u8>>`. `None` if any length > `u32::MAX`. Do **not** `as u32` truncate. Oversize ClientRequest → `ClientResp::Err(Invalid)`, **do not** bump `next_index`. |
| Fsync `Err` | **Keep** the pending log entry. `Reply(Err(Io))` means **uncertain**, not “forgotten.” A later covering Fsync `Ok` **applies** it. Do **not** restore `fsync_err_then_get_does_not_see_put` (that test encoded the bug). |
| Covering Fsync | Get (or any later op) whose Fsync `Ok` has `last_index` ≥ the failed Put **must** apply the Put. Live SM must match Recover from that prefix. |
| Fsync `Err` then crash | `Recover` from empty/old prefix → Put gone. That is the “lost Put” case. |
| Durable cookie | **Monotonic** on `last_index`. Never `node.durable = job.cookie`. Use `note_durable`: only advance if `cookie.last_index > durable.last_index`. Pipelined Fsync 2 then Fsync 1 must leave `last_index == 2`. |
| Append `Err` | Remove that pending entry; `Reply(Err(Io))`; do not apply it later. Node: on `write_all` fail, **truncate file back to pre-append offset**, then `IoComplete(Err)`. |
| In-flight dup `(client, request)` | Not a second Append. Increment `extra_replies`; Reply that many times when Fsync completes. If already `replied` (uncertain), immediate `Reply(Err(Io))`. After apply, idempotency table hits. |
| Node I/O | `FileDisk::submit` returns `Vec<Event>`, **not** `io::Result`. Every `IoSubmit` gets `IoComplete { Ok \| Err }`. Do not `?` out of the event loop. |
| Sim Fsync fail | `SimDisk.fail_next_fsync`. Failed Fsync does **not** move `durable_len`. Bytes stay. |
| Apply | Inside `step` after durable prefix advances. No `Effect::Apply`. |
| `commitIndex` | **Do not add in P1.** Durable cookie `last_index` only. |
| `PersistCookie` | Full struct now (`term`, `voted_for`, `last_index`, `last_term`). P1 fills index; term=0. |
| `Message` | `Ping` stub only. KV never `Send`s. |
| CRC | Hand-rolled CRC-32/ISO-HDLC, zlib poly `0xEDB88320`, of `len_le \|\| payload`. No CRC crate. |
| Collections | `BTreeMap`/`BTreeSet`/`Vec` in protocol **and** sim. |
| Cargo lints | Cargo 1.97 forbids mixing `lints.workspace = true` with extra keys. Clippy **deny** lives in **workspace** `Cargo.toml`; **lists** live in `crates/chronos-protocol/clippy.toml` and `crates/chronos-sim/clippy.toml`. Do not put `[lints.clippy]` on a crate that also has `workspace = true`. |

Core API: `fn step(node: &mut Node, event: Event) -> Vec<Effect>` and `Node::step`.

---

## What the code actually does (P1)

**Put/Get path:** `ClientRequest` → `IoSubmit(Append(Entry))` then `IoSubmit(Fsync)` → no Reply yet → `IoComplete(Append)` ignored unless `Err` → `IoComplete(Fsync, Ok)` apply + Reply → `IoComplete(Fsync, Err)` Reply Err, **keep pending**.

**Recover:** bump `incarnation`, reset local IoId, clear volatile, `scan(durable)`, replay Meta/Entry into map + idempotency, `next_index` after last entry.

**Sim disk:** `bytes` + `durable_len`. Append mutates `bytes` immediately (emission order). Fsync snapshots `len` at submit; `durable_len` moves when the **Ok** completion is **popped**. `crash()` drops the completion queue and truncates `bytes` to `durable_len`. Completions are a delay-0 `VecDeque` (not the P2 heap).

**Node disk:** one file. `load_and_truncate` CRC-scans and `set_len`. **Crash semantics ≠ sim:** node Recover sees CRC-valid `read()` bytes, which can include writes visible without fsync. Sim crash uses `durable_len` only. Documented in `chronos-node/src/disk.rs`. Do not claim crash-equivalence. Happy-path swap (same ClientRequests → same Replies) is the P1 proof.

**CLI:** `chronos-node [path]` default `chronos.wal`; Recover; hardcoded Put k=v then Get. Not a product CLI.

---

## Tests that exist (must keep)

Protocol (`raft/step.rs`, `wal/record.rs`, `types.rs`, `event.rs`, `codec.rs`):

- `IoId: Eq + Ord`
- exhaustive Event classify
- WAL: empty, one Meta, two records, torn 3 bytes, flipped CRC, huge len, Meta none roundtrip
- CRC empty + `"123456789"` → `0xCBF43926`
- Put then fsync Ok then Get sees value
- **Fsync Err then covering Get sees Put** (`fsync_err_then_covering_get_sees_put`)
- **Fsync Err then Recover empty loses Put**
- crash before fsync Recover loses Put
- duplicate after apply → Reply, no Append
- duplicate in-flight → extra Replies on Fsync Ok
- Recover replays durable Entry
- stale `IoComplete` after Recover ignored
- pipelined Fsync reorder does not regress `last_index`
- Append Err does not apply

Sim: Put/Get; covering Fsync after fail matches Recover; crash-before-fsync drops Put.

Node: Put/Get on a temp file.

Gates: `crates/chronos-protocol/tests/determinism_gates.rs`; `scripts/check-determinism-gates.ps1` / `.sh`.

**Do not** reintroduce a test that expects Get `NotFound` after Fsync Err **without** Recover, if a later Fsync succeeded.

---

## P1 exit checklist vs reality

| Check | Status |
|-------|--------|
| No Raft RPCs, no election | Done (empty `election`/`replication`/`log` modules) |
| No Tokio | Done |
| WAL encode/decode/scan tests | Written; **not executed** (link) |
| Put/Get via step + fsync | Written; not executed |
| Recover drops unsynced Put | Written; not executed |
| Sim stub and node disk same `step` | Written; not executed |
| Clippy + check | **Pass** (2026-08-19) |

`docs/roadmap/P01-foundation.md` still says **Status: not started**. Update that file when you mark P1 done after `cargo test`.

---

## Still open (not blockers for *writing* P2, **are** blockers for *starting* P2)

**Must before P2**

1. Install VS 2022 Build Tools (C++ + Windows SDK) so `link.exe` / `kernel32.lib` exist. Unset leftover `RUSTFLAGS=-C linker=rust-lld` if it is in the environment — it was used as a failed workaround.
2. `cargo test --workspace` green.
3. Optional sanity: `chronos-node` on a temp WAL prints Put/Get Ok.

**Known leftovers (do not “fix” into P2 scope unless they bite)**

- No file lock on the WAL; two processes on one file will corrupt it.
- No injected real Windows `FlushFileBuffers` failure test (protocol/sim cover the logic).
- No 4 GiB encode fixture (encode returns `None`; don’t allocate 4 GiB in CI).
- `saturating_add` on incarnation / `next_index` (pathological).
- Unbounded idempotency table (D8; bound sim length in P6, not snapshots).
- Node Recover ≠ sim `durable_len` crash (accepted; comment on `FileDisk`).
- `docs/adr/` never created (D1–D17 live in architecture; P1 Recover/bytes/dups were going to be ADR-0001–0003 — user did not confirm writing them).
- Artifact path `docs/bugs/` vs `docs/findings/` — unresolved.
- `docs/plan/phase-04-faults.md` — stale; ignore; use `roadmap/P04-faults.md`.
- Independent SM oracle not a named P5 step.

---

## Locked design D1–D17 (do not reopen)

Full prose: `docs/02-architecture.md` § Locked decisions.

| ID | Lock |
|----|------|
| D1 | Rust |
| D2 | Own single-thread scheduler. **No Tokio, MadSim, Turmoil.** |
| D3 | Effects + completions. Persist-before-send/ack **only** for votes and AE-success. Leader **may** `Append`+`Send` AE in one batch. |
| D4 | Tickless `u64` ns. Heap key `(time, global seq)` — one scheduler counter, not per-node. |
| D5 | Three crates. sim ↛ node, node ↛ sim. |
| D6 | BTree in protocol **and** sim. HashMap is a determinism bug. |
| D7 | Get via log in v1. |
| D8 | `(client_id, request_id)` on every op; table is SM state. |
| D9 | Checksummed WAL; torn suffix **past** `durable_len` only. |
| D10 | Fsync-lie **off** by default. |
| D11 | Static membership, no snapshots. |
| D12 | Two checkers; apply inside `step`; no `Effect::Apply`. |
| D13 | SHA-256 of encoded `TraceRecord`s. No `std::hash::Hasher`, no host fields. |
| D14 | LE `[len][crc32(len\|\|payload)][payload]`. |
| D15 | `ArmTimer { kind }`. Protocol has no `now`, no RNG. |
| D16 | Clippy lists + CI grep. Cargo does not catch `use std::fs`. |
| D17 | WAL `Meta` + `Entry`. Appends to one node in emission order. Completions may reorder. |

**IoId:** `{ incarnation, local }`. Recover bumps incarnation, resets local. Stale completions ignored.

**MsgId:** harness at enqueue, not on `Effect::Send`.

---

## Current tree (what is real vs stub)

```text
c:\projects\Chronos\
  Cargo.toml                 workspace; unsafe_code forbid; clippy disallowed_* deny
  clippy.toml                comment only (do not put Instant/fs bans here — node needs them)
  scripts/check-determinism-gates.{ps1,sh}
  crates/chronos-protocol/   IMPLEMENTED P1 (+ review fixes)
    src/{lib,types,event,effect,codec}.rs
    src/wal/{mod,record}.rs
    src/kv/{mod,machine}.rs
    src/raft/{mod,state,step}.rs     step+state live; election/log/replication STILL STUBS
    tests/determinism_gates.rs
    clippy.toml              HashMap/Instant/fs/net/thread
  crates/chronos-sim/        P1 stub disk+drive IMPLEMENTED; rest STUBS
    src/{disk,cluster}.rs    live
    src/{rng,scheduler,net,trace,check,history,fuzz,minify,main}.rs  stubs
    clippy.toml              HashMap/HashSet
  crates/chronos-node/       P1 file WAL IMPLEMENTED
    src/{main,disk}.rs       live
    src/{net,timer}.rs       stubs
```

P2 fills `rng`, `scheduler`, `net`, `disk` (torn tail + delay), `trace` (SHA-256), toy or P1 KV in the heap loop. File: `docs/roadmap/P02-simulator.md`. **Blocked on `cargo test` green.**

---

## Environment (this machine)

- Workspace: `c:\projects\Chronos`
- Shell: PowerShell. `Set-PSReadLineOption` profile errors are noise.
- Rust: `C:\Users\sahil\.cargo\bin\cargo.exe` — **1.97.1** after a broken partial install was uninstalled and `rustup toolchain install stable` succeeded. Host: `x86_64-pc-windows-msvc`.
- **No Visual Studio C++ / Windows SDK.** `link.exe` missing. `rust-lld` fails on `kernel32.lib`. Check/clippy work; **test binaries do not**.
- `winget install Rustlang.Rustup` hung/failed; `rustup-init.exe` in repo dir is gitignored.
- Target: Windows + Linux later (P9). Codec must be identical.

Commands that **passed** here:

```text
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --tests --bins
powershell -NoProfile -File scripts\check-determinism-gates.ps1
```

Command that **fails** until SDK exists: `cargo test --workspace`.

---

## Hard “do not”

- Do not add Tokio / MadSim / Turmoil.
- Do not put `HashMap`/`HashSet` in protocol or sim.
- Do not call `Instant`/`SystemTime`/`std::fs`/`std::net`/`thread::spawn` from protocol.
- Do not implement election, AppendEntries, or a 3-node cluster until P3.
- Do not start P2 until `cargo test --workspace` is green (user can override; default is no).
- Do not turn fsync-lie on in default tests.
- Do not reorder two `Append`s to the same node’s byte buffer.
- Do not drop pending on Fsync `Err` (that was the core-review blocker).
- Do not assign `durable = cookie` (non-monotonic).
- Do not make `FileDisk::submit` return `io::Result` and abort the loop.
- Do not rewrite `docs/00-project-guide.md`; 02 wins.
- Do not create a fourth crate until it hurts.
- Do not git commit unless the user asks.
- Do not backfill 17 ADR files unless asked; constitution is `02-architecture.md`.

---

## Next chat: do this

1. `cargo test --workspace` (after Build Tools if needed). Fix any real test failures; do not weaken covering-Fsync tests.
2. If P1 tests pass, mark P01 exit checklist done (optional doc edit).
3. **P2** per [docs/roadmap/P02-simulator.md](roadmap/P02-simulator.md): seeded RNG, tickless heap `(time, global seq)`, sim net, extend sim disk (delay + torn tail later/P4), SHA-256 of encoded traces, `hash(run(seed))==hash(run(seed))`. Toy protocol or this P1 KV. **No Raft. No Tokio.**

---

## How to prompt the next chat (copy)

```text
Continue Chronos from docs/HANDOFF.md.
Architecture: docs/02-architecture.md wins all conflicts.
P1 is implemented including post-review durability fixes. Do not re-review or reimplement P1.
First: cargo test --workspace. If it cannot link, say so; do not fake green.
If tests pass: implement P2 per docs/roadmap/P02-simulator.md.
Do not implement Raft or Tokio.
Do not revert Fsync-Err-keeps-pending or monotonic durable last_index.
```

---

## v1 punchline (do not build now)

P7+P8: `docs/bugs/` (or findings — pick one later) with seed, commit, digest, `schedule.min`, RCA. Preferred: real swarm find. Fallback: labeled planted near-miss. P1–P6 exist only to make that possible.
