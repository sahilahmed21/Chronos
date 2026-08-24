//! Chronos protocol: `(State, Event) -> (State, Vec<Effect>)`.
//!
//! No I/O, no clock, no RNG. Interpreters live in `chronos-sim` and `chronos-node`.
//! Spec: `docs/02-architecture.md`.

pub mod codec;
pub mod effect;
pub mod event;
pub mod kv;
pub mod raft;
pub mod types;
pub mod wal;

pub use effect::{ClientError, ClientResp, Effect, IoOp, TimerKind};
pub use event::{ClientReq, Event, IoError, Message};
pub use raft::log::Log;
pub use raft::state::{Node, PersistCookie, Role, TIMER_ELECTION, TIMER_HEARTBEAT};
pub use raft::step::step;
pub use types::{ClientId, Cmd, Index, IoId, MsgId, NodeId, RequestId, Term, TimerId, Timestamp};
pub use wal::record::{
    decode_record, encode_record, scan, CodecError, LogEntry, LogPayload, WalRecord,
};
