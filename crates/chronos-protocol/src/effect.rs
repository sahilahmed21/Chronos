//! Effects a node asks of the world.
//!
//! Spec: `docs/02-architecture.md` § Effects.
//! Timers are `ArmTimer { kind }`, not absolute deadlines. No `Apply`.

use crate::event::Message;
use crate::types::{ClientId, IoId, NodeId, RequestId, TimerId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerKind {
    Election,
    Heartbeat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IoOp {
    Append { bytes: Vec<u8> },
    Fsync,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError {
    NotFound,
    Io,
    Invalid,
    NotLeader,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientResp {
    Ok { value: Vec<u8> },
    Err(ClientError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Send {
        to: NodeId,
        msg: Message,
    },
    IoSubmit {
        id: IoId,
        op: IoOp,
    },
    ArmTimer {
        id: TimerId,
        kind: TimerKind,
    },
    CancelTimer {
        id: TimerId,
    },
    Reply {
        to: ClientId,
        request: RequestId,
        resp: ClientResp,
    },
}
