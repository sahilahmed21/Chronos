//! Events the world delivers to a node.
//!
//! Spec: `docs/02-architecture.md` § Events.
//! `Recover` carries the durable WAL prefix. Crash drops `Node`; the interpreter
//! truncates the file and passes the prefix here.

use crate::codec::{put_u32_le, put_u64_le, read_u32_le, read_u64_le, read_u8};
use crate::types::{ClientId, Index, IoId, NodeId, RequestId, Term, TimerId};
use crate::wal::record::{decode_log_entry, encode_log_entry, LogEntry};

pub use crate::types::Cmd;

/// Raft RPC. `from` on `MessageReceived` is authoritative.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Ping,
    RequestVote {
        term: Term,
        last_log_index: Index,
        last_log_term: Term,
    },
    RequestVoteResp {
        term: Term,
        granted: bool,
    },
    AppendEntries {
        term: Term,
        prev_index: Index,
        prev_term: Term,
        entries: Vec<LogEntry>,
        leader_commit: Index,
    },
    AppendEntriesResp {
        term: Term,
        success: bool,
        match_index: Index,
    },
}

const MSG_PING: u8 = 0;
const MSG_REQUEST_VOTE: u8 = 1;
const MSG_REQUEST_VOTE_RESP: u8 = 2;
const MSG_APPEND_ENTRIES: u8 = 3;
const MSG_APPEND_ENTRIES_RESP: u8 = 4;

impl Message {
    pub fn encode(&self) -> Option<Vec<u8>> {
        let mut buf = Vec::new();
        match self {
            Message::Ping => buf.push(MSG_PING),
            Message::RequestVote {
                term,
                last_log_index,
                last_log_term,
            } => {
                buf.push(MSG_REQUEST_VOTE);
                put_u64_le(&mut buf, term.0);
                put_u64_le(&mut buf, last_log_index.0);
                put_u64_le(&mut buf, last_log_term.0);
            }
            Message::RequestVoteResp { term, granted } => {
                buf.push(MSG_REQUEST_VOTE_RESP);
                put_u64_le(&mut buf, term.0);
                buf.push(u8::from(*granted));
            }
            Message::AppendEntries {
                term,
                prev_index,
                prev_term,
                entries,
                leader_commit,
            } => {
                buf.push(MSG_APPEND_ENTRIES);
                put_u64_le(&mut buf, term.0);
                put_u64_le(&mut buf, prev_index.0);
                put_u64_le(&mut buf, prev_term.0);
                put_u32_le(&mut buf, u32::try_from(entries.len()).ok()?);
                for entry in entries {
                    encode_log_entry(entry, &mut buf)?;
                }
                put_u64_le(&mut buf, leader_commit.0);
            }
            Message::AppendEntriesResp {
                term,
                success,
                match_index,
            } => {
                buf.push(MSG_APPEND_ENTRIES_RESP);
                put_u64_le(&mut buf, term.0);
                buf.push(u8::from(*success));
                put_u64_le(&mut buf, match_index.0);
            }
        }
        Some(buf)
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        let mut pos = 0;
        let tag = read_u8(data, &mut pos)?;
        let msg = match tag {
            MSG_PING => Message::Ping,
            MSG_REQUEST_VOTE => {
                let term = Term(read_u64_le(data, &mut pos)?);
                let last_log_index = Index(read_u64_le(data, &mut pos)?);
                let last_log_term = Term(read_u64_le(data, &mut pos)?);
                Message::RequestVote {
                    term,
                    last_log_index,
                    last_log_term,
                }
            }
            MSG_REQUEST_VOTE_RESP => {
                let term = Term(read_u64_le(data, &mut pos)?);
                let granted = read_u8(data, &mut pos)? != 0;
                Message::RequestVoteResp { term, granted }
            }
            MSG_APPEND_ENTRIES => {
                let term = Term(read_u64_le(data, &mut pos)?);
                let prev_index = Index(read_u64_le(data, &mut pos)?);
                let prev_term = Term(read_u64_le(data, &mut pos)?);
                let n = read_u32_le(data, &mut pos)? as usize;
                let mut entries = Vec::with_capacity(n);
                for _ in 0..n {
                    entries.push(decode_log_entry(data, &mut pos)?);
                }
                let leader_commit = Index(read_u64_le(data, &mut pos)?);
                Message::AppendEntries {
                    term,
                    prev_index,
                    prev_term,
                    entries,
                    leader_commit,
                }
            }
            MSG_APPEND_ENTRIES_RESP => {
                let term = Term(read_u64_le(data, &mut pos)?);
                let success = read_u8(data, &mut pos)? != 0;
                let match_index = Index(read_u64_le(data, &mut pos)?);
                Message::AppendEntriesResp {
                    term,
                    success,
                    match_index,
                }
            }
            _ => return None,
        };
        if pos != data.len() {
            return None;
        }
        Some(msg)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientReq {
    pub client: ClientId,
    pub request: RequestId,
    pub cmd: Cmd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoError {
    FsyncFailed,
    IoFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    TimerFired {
        timer: TimerId,
    },
    MessageReceived {
        from: NodeId,
        msg: Message,
    },
    IoComplete {
        id: IoId,
        result: Result<(), IoError>,
    },
    Recover {
        durable: Vec<u8>,
    },
    ClientRequest {
        req: ClientReq,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ClientId, IoId, NodeId, RequestId, TimerId};
    use crate::wal::record::{LogEntry, LogPayload};

    fn classify(e: &Event) -> u8 {
        match e {
            Event::TimerFired { .. } => 0,
            Event::MessageReceived { .. } => 1,
            Event::IoComplete { .. } => 2,
            Event::Recover { .. } => 3,
            Event::ClientRequest { .. } => 4,
        }
    }

    #[test]
    fn classify_is_exhaustive() {
        assert_eq!(classify(&Event::TimerFired { timer: TimerId(0) }), 0);
        assert_eq!(
            classify(&Event::MessageReceived {
                from: NodeId(1),
                msg: Message::Ping,
            }),
            1
        );
        assert_eq!(
            classify(&Event::IoComplete {
                id: IoId {
                    incarnation: 0,
                    local: 0
                },
                result: Ok(()),
            }),
            2
        );
        assert_eq!(classify(&Event::Recover { durable: vec![] }), 3);
        assert_eq!(
            classify(&Event::ClientRequest {
                req: ClientReq {
                    client: ClientId(1),
                    request: RequestId(1),
                    cmd: Cmd::Get { key: b"k".to_vec() },
                }
            }),
            4
        );
    }

    #[test]
    fn message_roundtrip_request_vote() {
        let msg = Message::RequestVote {
            term: Term(4),
            last_log_index: Index(7),
            last_log_term: Term(3),
        };
        let bytes = msg.encode().unwrap();
        assert_eq!(Message::decode(&bytes), Some(msg));
    }

    #[test]
    fn message_roundtrip_append_entries_with_noop() {
        let msg = Message::AppendEntries {
            term: Term(2),
            prev_index: Index(0),
            prev_term: Term(0),
            entries: vec![LogEntry {
                term: Term(2),
                payload: LogPayload::NoOp,
            }],
            leader_commit: Index(1),
        };
        let bytes = msg.encode().unwrap();
        assert_eq!(Message::decode(&bytes), Some(msg));
    }

    #[test]
    fn message_roundtrip_append_entries_resp() {
        let msg = Message::AppendEntriesResp {
            term: Term(2),
            success: true,
            match_index: Index(3),
        };
        let bytes = msg.encode().unwrap();
        assert_eq!(Message::decode(&bytes), Some(msg));
    }
}
