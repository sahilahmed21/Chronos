//! Persistent + volatile node state and in-flight I/O (`PersistCookie`, `IoId` incarnation).
//!
//! Spec: `docs/02-architecture.md`.

use std::collections::{BTreeMap, BTreeSet};

use crate::effect::{ClientResp, Effect, IoOp, TimerKind};
use crate::raft::log::Log;
use crate::types::{ClientId, Index, IoId, NodeId, RequestId, Term, TimerId};
use crate::wal::record::{encode_record, WalRecord};

pub const TIMER_ELECTION: TimerId = TimerId(0);
pub const TIMER_HEARTBEAT: TimerId = TimerId(1);

/// Cookie the checker (P5) compares against outgoing vote/ack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PersistCookie {
    pub term: Term,
    pub voted_for: Option<NodeId>,
    pub last_index: Index,
    pub last_term: Term,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Follower,
    Candidate,
    Leader,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum VoteWork {
    Campaign,
    Reply { to: NodeId, granted: bool },
    Silent,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AeAck {
    pub to: NodeId,
    pub success: bool,
    pub match_index: Index,
    pub leader_commit: Index,
    pub log_gen: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum IoKind {
    AppendMeta { fsync_id: IoId },
    AppendLog { from: Index, fsync_id: IoId },
    VoteFsync { work: VoteWork },
    LogFsync { ack: Option<AeAck> },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct InFlight {
    pub cookie: PersistCookie,
    pub kind: IoKind,
}

pub(crate) struct PersistSpec {
    pub meta: bool,
    pub truncate_to: Option<Index>,
    pub log_from: Option<Index>,
    pub fsync: IoKind,
}

#[derive(Clone, Debug)]
pub(crate) struct Pending {
    pub index: Index,
    pub client: ClientId,
    pub request: RequestId,
    pub replied: bool,
    pub extra_replies: u32,
}

pub struct Node {
    pub(crate) id: NodeId,
    pub(crate) peers: Vec<NodeId>,
    pub(crate) role: Role,
    pub(crate) current_term: Term,
    pub(crate) voted_for: Option<NodeId>,
    pub(crate) log: Log,
    pub(crate) commit_index: Index,
    pub(crate) last_applied: Index,
    pub(crate) next_index: BTreeMap<NodeId, Index>,
    pub(crate) match_index: BTreeMap<NodeId, Index>,
    pub(crate) match_index_self: Index,
    pub(crate) votes: BTreeSet<NodeId>,
    pub(crate) log_gen: u64,
    pub(crate) store: BTreeMap<Vec<u8>, Vec<u8>>,
    pub(crate) idempotency: BTreeMap<(ClientId, RequestId), ClientResp>,
    pub(crate) in_flight: BTreeMap<IoId, InFlight>,
    pub(crate) durable: PersistCookie,
    pub(crate) incarnation: u64,
    pub(crate) next_local: u64,
    pub(crate) pending: Vec<Pending>,
    /// Sim buggify: nack a matching AppendEntries. Default false. No RNG.
    pub(crate) reject_ok_ae: bool,
    /// Test hook: Send RequestVote / grant in the persist batch. Default false.
    pub(crate) skip_vote_persist: bool,
}

impl Node {
    pub fn new(id: NodeId, peers: Vec<NodeId>) -> Self {
        Self {
            id,
            peers,
            role: Role::Follower,
            current_term: Term(0),
            voted_for: None,
            log: Log::new(),
            commit_index: Index(0),
            last_applied: Index(0),
            next_index: BTreeMap::new(),
            match_index: BTreeMap::new(),
            match_index_self: Index(0),
            votes: BTreeSet::new(),
            log_gen: 0,
            store: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            in_flight: BTreeMap::new(),
            durable: PersistCookie::default(),
            incarnation: 0,
            next_local: 0,
            pending: Vec::new(),
            reject_ok_ae: false,
            skip_vote_persist: false,
        }
    }

    pub fn set_reject_ok_ae(&mut self, yes: bool) {
        self.reject_ok_ae = yes;
    }

    pub fn set_skip_vote_persist(&mut self, yes: bool) {
        self.skip_vote_persist = yes;
    }

    pub fn match_index_self(&self) -> Index {
        self.match_index_self
    }

    pub fn kv_store(&self) -> &BTreeMap<Vec<u8>, Vec<u8>> {
        &self.store
    }

    pub fn idempotency(&self) -> &BTreeMap<(ClientId, RequestId), ClientResp> {
        &self.idempotency
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn commit_index(&self) -> Index {
        self.commit_index
    }

    pub fn last_applied(&self) -> Index {
        self.last_applied
    }

    pub fn last_log_index(&self) -> Index {
        self.log.last_index()
    }

    pub fn last_log_term(&self) -> Term {
        self.log.last_term()
    }

    pub fn current_term(&self) -> Term {
        self.current_term
    }

    pub fn voted_for(&self) -> Option<NodeId> {
        self.voted_for
    }

    pub fn durable(&self) -> PersistCookie {
        self.durable
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    pub fn kv_get(&self, key: &[u8]) -> Option<&[u8]> {
        self.store.get(key).map(|v| v.as_slice())
    }

    pub(crate) fn cluster_size(&self) -> usize {
        1 + self.peers.len()
    }

    pub(crate) fn is_majority(&self, n: usize) -> bool {
        n * 2 > self.cluster_size()
    }

    pub(crate) fn cookie(&self) -> PersistCookie {
        PersistCookie {
            term: self.current_term,
            voted_for: self.voted_for,
            last_index: self.log.last_index(),
            last_term: self.log.last_term(),
        }
    }

    pub(crate) fn alloc_io(&mut self) -> IoId {
        let id = IoId {
            incarnation: self.incarnation,
            local: self.next_local,
        };
        self.next_local = self.next_local.saturating_add(1);
        id
    }

    pub(crate) fn is_member(&self, id: NodeId) -> bool {
        id == self.id || self.peers.contains(&id)
    }

    pub(crate) fn bump_log_gen(&mut self) {
        self.log_gen = self.log_gen.saturating_add(1);
    }

    pub(crate) fn ae_ack(
        &self,
        to: NodeId,
        success: bool,
        match_index: Index,
        leader_commit: Index,
    ) -> AeAck {
        AeAck {
            to,
            success,
            match_index,
            leader_commit,
            log_gen: self.log_gen,
        }
    }

    pub(crate) fn note_durable_index(&mut self, last_index: Index, last_term: Term) {
        let last_index = last_index.min(self.log.last_index());
        if last_index > self.durable.last_index {
            self.durable.last_index = last_index;
            self.durable.last_term = if last_index == self.log.last_index() {
                self.log.last_term()
            } else {
                last_term
            };
        }
    }

    pub(crate) fn note_durable_meta(&mut self, cookie: PersistCookie) {
        if cookie.term > self.durable.term {
            self.durable.term = cookie.term;
            self.durable.voted_for = cookie.voted_for;
        } else if cookie.term == self.durable.term {
            self.durable.voted_for = cookie.voted_for;
        }
        self.note_durable_index(cookie.last_index, cookie.last_term);
    }

    pub(crate) fn arm_election(effects: &mut Vec<Effect>) {
        effects.push(Effect::ArmTimer {
            id: TIMER_ELECTION,
            kind: TimerKind::Election,
        });
    }

    pub(crate) fn arm_heartbeat(effects: &mut Vec<Effect>) {
        effects.push(Effect::ArmTimer {
            id: TIMER_HEARTBEAT,
            kind: TimerKind::Heartbeat,
        });
    }

    pub(crate) fn cancel_election(effects: &mut Vec<Effect>) {
        effects.push(Effect::CancelTimer { id: TIMER_ELECTION });
    }

    pub(crate) fn cancel_heartbeat(effects: &mut Vec<Effect>) {
        effects.push(Effect::CancelTimer {
            id: TIMER_HEARTBEAT,
        });
    }

    pub(crate) fn step_down_to(&mut self, term: Term, effects: &mut Vec<Effect>) {
        let was_leader = self.role == Role::Leader;
        self.current_term = term;
        self.voted_for = None;
        self.role = Role::Follower;
        self.votes.clear();
        self.next_index.clear();
        self.match_index.clear();
        if was_leader {
            Self::cancel_heartbeat(effects);
        }
        Self::arm_election(effects);
    }

    /// Persist Meta, optional Truncate, and/or log entries, then one Fsync.
    /// Returns whether I/O was submitted. Encodes fully before submitting.
    pub(crate) fn persist(&mut self, spec: PersistSpec, effects: &mut Vec<Effect>) -> bool {
        let cookie = self.cookie();
        let meta_bytes = if spec.meta {
            match encode_record(&WalRecord::Meta {
                term: self.current_term,
                voted_for: self.voted_for,
            }) {
                Some(bytes) => Some(bytes),
                None => return false,
            }
        } else {
            None
        };

        let mut tail = Vec::new();
        if let Some(idx) = spec.truncate_to {
            let Some(bytes) = encode_record(&WalRecord::Truncate { index: idx }) else {
                return false;
            };
            tail.extend_from_slice(&bytes);
        }
        if let Some(from) = spec.log_from {
            let last = self.log.last_index().0;
            for i in from.0..=last {
                let Some(entry) = self.log.entry(Index(i)) else {
                    continue;
                };
                let Some(rec) = encode_record(&WalRecord::Entry(entry.clone())) else {
                    return false;
                };
                tail.extend_from_slice(&rec);
            }
        }
        if meta_bytes.is_none() && tail.is_empty() {
            return false;
        }

        let log_from = spec
            .log_from
            .or_else(|| spec.truncate_to.map(|idx| Index(idx.0.saturating_add(1))));
        let need_meta = meta_bytes.is_some();
        let need_log = !tail.is_empty();
        let meta_id = if need_meta {
            Some(self.alloc_io())
        } else {
            None
        };
        let log_id = if need_log {
            Some(self.alloc_io())
        } else {
            None
        };
        let fsync_id = self.alloc_io();

        if let (Some(bytes), Some(append_id)) = (meta_bytes, meta_id) {
            self.in_flight.insert(
                append_id,
                InFlight {
                    cookie,
                    kind: IoKind::AppendMeta { fsync_id },
                },
            );
            effects.push(Effect::IoSubmit {
                id: append_id,
                op: IoOp::Append { bytes },
            });
        }
        if let (Some(append_id), Some(from)) = (log_id, log_from) {
            self.in_flight.insert(
                append_id,
                InFlight {
                    cookie,
                    kind: IoKind::AppendLog { from, fsync_id },
                },
            );
            effects.push(Effect::IoSubmit {
                id: append_id,
                op: IoOp::Append { bytes: tail },
            });
        }
        self.in_flight.insert(
            fsync_id,
            InFlight {
                cookie,
                kind: spec.fsync,
            },
        );
        effects.push(Effect::IoSubmit {
            id: fsync_id,
            op: IoOp::Fsync,
        });
        true
    }

    pub(crate) fn persist_fsync_only(&mut self, fsync: IoKind, effects: &mut Vec<Effect>) {
        let cookie = self.cookie();
        let fsync_id = self.alloc_io();
        self.in_flight.insert(
            fsync_id,
            InFlight {
                cookie,
                kind: fsync,
            },
        );
        effects.push(Effect::IoSubmit {
            id: fsync_id,
            op: IoOp::Fsync,
        });
    }

    pub(crate) fn log_fsync_covers(&self, match_index: Index) -> bool {
        self.in_flight.values().any(|job| match job.kind {
            IoKind::LogFsync { ack: Some(ack) } => {
                ack.log_gen == self.log_gen && ack.success && ack.match_index >= match_index
            }
            IoKind::LogFsync { ack: None } => job.cookie.last_index >= match_index,
            _ => false,
        })
    }

    pub(crate) fn drop_paired_fsync(&mut self, fsync_id: IoId) {
        self.in_flight.remove(&fsync_id);
    }
}

impl Default for Node {
    fn default() -> Self {
        Self::new(NodeId(0), Vec::new())
    }
}
