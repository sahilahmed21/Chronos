# P1 — Foundation

**Status:** implemented  
**Serves:** G1, G2, G5.1–G5.2  
**Depends on:** scaffold crates  
**Must not:** implement Raft, Tokio, multi-node, partitions, SHA-256 swarm

**Done when:** a single-node WAL KV talks only through `step` + effects; swapping the sim stub interpreter and the real node interpreter does **not** change KV logic.

**Files:** `crates/chronos-protocol/src/{types,event,effect,codec,kv/machine,wal/record,raft/step}.rs`, `crates/chronos-node/src/{main,disk,timer}.rs`, `crates/chronos-sim/src/{cluster,disk,rng}.rs` (stub interpreter only).

---

## P1.A — Core types

### P1.A.1 Newtypes and IDs

- **P1.A.1.a** `Timestamp(u64)`, `NodeId(u8)`, `Term(u64)`, `Index(u64)`, `ClientId(u64)`, `RequestId(u64)`, `TimerId(u64)`, `MsgId(u64)` in `types.rs`.
- **P1.A.1.b** `IoId { incarnation: u64, local: u64 }`. Document: local resets on Recover; incarnation never goes backward.
- **P1.A.1.c** No `f64`. No `std::time`. Use `BTreeMap`/`BTreeSet`/`Vec` only.

**Exit:** types compile; a one-line test that `IoId` is `Eq + Ord`.

### P1.A.2 Events

- **P1.A.2.a** `Event`: `TimerFired { timer }`, `MessageReceived { from, msg }`, `IoComplete { id, result }`, `Recover`, `ClientRequest { req }`.
- **P1.A.2.b** `ClientReq { client, request, cmd }`. `Cmd` for P1: `Get { key }`, `Put { key, value }`. No Raft RPCs yet (or define `Message` as `#[allow]` stub enum with a `Ping` only).
- **P1.A.2.c** `IoError` with at least `FsyncFailed` / `IoFailed`.

**Exit:** exhaustive match in a dummy `fn classify(e: &Event)`.

### P1.A.3 Effects and IoOp

- **P1.A.3.a** `Effect`: `Send`, `IoSubmit { id, op }`, `ArmTimer { id, kind }`, `CancelTimer { id }`, `Reply { to, resp }`. **No** `Apply`. **No** `SetTimer { at }`.
- **P1.A.3.b** `TimerKind { Election, Heartbeat }`.
- **P1.A.3.c** `IoOp { Append { bytes }, Fsync }`.
- **P1.A.3.d** `ClientResp { Ok(Value) | Err(...) }`.

**Exit:** `step` signature exists even if it only handles `ClientRequest`/`IoComplete`/`Recover`.

---

## P1.B — WAL records (G5.1, D14, D17)

### P1.B.1 Codec

- **P1.B.1.a** Little-endian `u32`/`u64` helpers in `codec.rs`. No serde.
- **P1.B.1.b** CRC-32/ISO-HDLC (zlib poly `0xEDB88320`) of `payload_len.to_le_bytes() || payload`. Prefer a tiny function in protocol (allowed) over a CRC crate if the crate pulls HashMap. A `crc32fast`-free 20-line table is fine.

### P1.B.2 Record format

- **P1.B.2.a** On-disk: `[u32 payload_len][u32 crc32][payload]`.
- **P1.B.2.b** Payload variants: `Meta { term, voted_for }` and `Entry { ... }` (P1 Entry can be a KV command blob). Encode a discriminant byte first.
- **P1.B.2.c** `encode_record` / `decode_record`. Decode fails if header does not fit or CRC mismatches.

### P1.B.3 Scan

- **P1.B.3.a** `scan(bytes) -> (records, valid_len)`. Stop at first bad header/CRC. `valid_len` is the truncation point.
- **P1.B.3.b** Tests: empty buffer; one Meta; two records; torn last 3 bytes; flipped CRC; huge `len` that does not fit.

**Exit:** those tests pass on protocol crate alone.

---

## P1.C — Single-node KV via step

Do **not** elect a leader. One node. Client ops go to a local log that is the WAL.

### P1.C.1 State

- **P1.C.1.a** `Node` in `raft/state.rs` (even pre-Raft): in-memory map, idempotency `BTreeMap<(ClientId, RequestId), ClientResp>`, in-flight `BTreeMap<IoId, PersistCookie>` (cookie can be “kv-log-len” for P1), `durable` cookie, `incarnation`, next `local` IoId, next TimerId.
- **P1.C.1.b** Apply is inside `step` after durable commit of that entry (for P1: after fsync of the Entry completes). Idempotency: duplicate request returns stored resp, no second mutation.

### P1.C.2 Put / Get path

- **P1.C.2.a** `ClientRequest(Put)`: allocate `IoId`, emit `IoSubmit(Append(Entry))` then `IoSubmit(Fsync)`. **No** `Reply` yet.
- **P1.C.2.b** `IoComplete(Append)`: ignore or track; do not treat as durable.
- **P1.C.2.c** `IoComplete(Fsync, Ok)`: advance durable, apply, `Reply`.
- **P1.C.2.d** `IoComplete(Fsync, Err)`: no apply, no Reply success (Reply err is ok).
- **P1.C.2.e** `Get` is also an Entry in the log (D7), same persist path. Do not read the in-memory map without going through apply of a logged Get. (Single-node this looks heavy; it is the API we keep for P3.)

### P1.C.3 Recover

- **P1.C.3.a** `Recover`: bump incarnation, clear in-flight, `scan` durable bytes, replay Meta/Entry into map + idempotency table, reset volatile.

**Exit:** unit tests with a fake interpreter in `chronos-protocol` tests: Put then fsync Ok → Get sees value; fsync Err → Get does not; crash before fsync → Recover loses the Put.

---

## P1.D — Two interpreters (G2)

### P1.D.1 Stub sim interpreter

- **P1.D.1.a** In `chronos-sim`, a loop that is **not** yet the full scheduler: apply `Append` to an in-memory `bytes` immediately; complete `Fsync` synchronously **or** as a queued event with delay 0. Prefer queued even in P1 so P2 does not rewrite.
- **P1.D.1.b** `ArmTimer`: ignore or fire never in P1 (single-node KV may not need timers). If ignored, document it.

### P1.D.2 Real node interpreter

- **P1.D.2.a** `chronos-node/disk.rs`: append bytes to a file; on Fsync call `FlushFileBuffers` (Windows) / `fdatasync` (Linux). Same record bytes protocol produced.
- **P1.D.2.b** Completions: after the syscall returns, feed `IoComplete` into `step`. Single-threaded.
- **P1.D.2.c** `main.rs`: open a data file, Recover, then a tiny CLI or hardcoded Put/Get to prove the loop.

### P1.D.3 Swap proof

- **P1.D.3.a** Same sequence of `ClientRequest`s against stub sim disk and against real file. Same replies. KV/`step` not `cfg`-switched.
- **P1.D.3.b** Do not share code between sim and node except protocol.

**Exit:** one test (sim) + one manual or ignored-by-default node test that writes a temp file.

---

## P1.E — Lint gate (G1.2 start)

### P1.E.1

- **P1.E.1.a** Confirm `crates/chronos-protocol/clippy.toml` and `crates/chronos-sim/clippy.toml` still match D6/D15/D16.
- **P1.E.1.b** Add `scripts/check-determinism-gates.sh` or a tiny `tests` that is documentation-only until P9 CI: `rg "std::collections::HashMap" crates/chronos-protocol` must be empty (except comments).

**Exit:** `cargo test -p chronos-protocol` green. `cargo test -p chronos-sim` may be empty.

---

## Phase exit checklist

- [x] No Raft RPCs, no leader election *(at P1 exit; Raft added in P3)*
- [x] No Tokio
- [x] WAL encode/decode/scan tests
- [x] Put/Get via step + fsync complete
- [x] Recover drops unsynced Put
- [x] Sim stub and node disk both drive the same `step`
