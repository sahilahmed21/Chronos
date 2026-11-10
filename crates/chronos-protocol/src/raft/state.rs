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

/// Deliberate protocol bugs used to measure checker detection power (ADR-0002).
///
/// The type always exists, but the bug behavior is compiled in only with the
/// `mutants` feature; `Node::set_mutant(Some(_))` panics without it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutant {
    /// Send/grant votes before the term+votedFor fsync completes.
    SkipVotePersist,
    /// Leader commits old-term entries by counting replicas (Raft Figure 8).
    CommitOldTerm,
    /// Leader answers `Get` from memory without going through the log.
    GetFromMemory,
    /// State machine ignores the idempotency table.
    DropIdempotency,
    /// Follower acks `AppendEntries` before its fsync completes.
    AckBeforeFsync,
    /// New leader does not append the current-term no-op entry.
    NoLeaderNoop,
}

impl Mutant {
    pub const ALL: [Mutant; 6] = [
        Mutant::SkipVotePersist,
        Mutant::CommitOldTerm,
        Mutant::GetFromMemory,
        Mutant::DropIdempotency,
        Mutant::AckBeforeFsync,
        Mutant::NoLeaderNoop,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Mutant::SkipVotePersist => "skip-vote-persist",
            Mutant::CommitOldTerm => "commit-old-term",
            Mutant::GetFromMemory => "get-from-memory",
            Mutant::DropIdempotency => "drop-idempotency",
            Mutant::AckBeforeFsync => "ack-before-fsync",
            Mutant::NoLeaderNoop => "no-leader-noop",
        }
    }

    pub fn from_name(name: &str) -> Option<Mutant> {
        Mutant::ALL.into_iter().find(|m| m.name() == name)
    }
}

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
    /// Deliberate bug, effective only with the `mutants` feature (ADR-0002).
    pub(crate) mutant: Option<Mutant>,
}

impl Node {
    /// Creates a new Raft node with the given ID and peer list.
    ///
    /// Initializes the node as a Follower at term 0 with an empty log and storage.
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
            mutant: None,
        }
    }

    /// Sets whether the node should simulate buggify by rejecting valid `AppendEntries`.
    pub fn set_reject_ok_ae(&mut self, yes: bool) {
        self.reject_ok_ae = yes;
    }

    /// Selects a deliberate bug. Panics if `Some` is passed in a build without the
    /// `mutants` feature, so a production binary can never run with one active.
    pub fn set_mutant(&mut self, mutant: Option<Mutant>) {
        assert!(
            mutant.is_none() || cfg!(feature = "mutants"),
            "built without the `mutants` feature"
        );
        self.mutant = mutant;
    }

    /// Returns the active mutant (always `None` without the `mutants` feature).
    pub fn mutant(&self) -> Option<Mutant> {
        self.mutant
    }

    /// True when `m` is active. Const-false without the feature, so hook code is dead there.
    pub(crate) fn is_mutant(&self, m: Mutant) -> bool {
        cfg!(feature = "mutants") && self.mutant == Some(m)
    }

    /// Compatibility wrapper for the planted ElectionSafety pack.
    pub fn set_skip_vote_persist(&mut self, yes: bool) {
        self.set_mutant(yes.then_some(Mutant::SkipVotePersist));
    }

    /// Returns the node's own match index (highest log entry replicated to itself).
    pub fn match_index_self(&self) -> Index {
        self.match_index_self
    }

    /// Returns a reference to the in-memory key-value store.
    pub fn kv_store(&self) -> &BTreeMap<Vec<u8>, Vec<u8>> {
        &self.store
    }

    /// Returns a reference to the idempotency cache.
    pub fn idempotency(&self) -> &BTreeMap<(ClientId, RequestId), ClientResp> {
        &self.idempotency
    }

    /// Returns the ID of the node.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Returns the current role of the node (Leader, Follower, or Candidate).
    pub fn role(&self) -> Role {
        self.role
    }

    /// Returns the node's commit index.
    pub fn commit_index(&self) -> Index {
        self.commit_index
    }

    /// Returns the highest log index applied to the state machine.
    pub fn last_applied(&self) -> Index {
        self.last_applied
    }

    /// Returns the last log index present in the node's log.
    pub fn last_log_index(&self) -> Index {
        self.log.last_index()
    }

    /// Returns the term of the last log entry present in the node's log.
    pub fn last_log_term(&self) -> Term {
        self.log.last_term()
    }

    /// Returns the node's current term.
    pub fn current_term(&self) -> Term {
        self.current_term
    }

    /// Returns the node ID that this node voted for in the current term, if any.
    pub fn voted_for(&self) -> Option<NodeId> {
        self.voted_for
    }

    /// Returns the node's durable state (persisted term, vote, log index/term).
    pub fn durable(&self) -> PersistCookie {
        self.durable
    }

    /// Returns a reference to the node's log.
    pub fn log(&self) -> &Log {
        &self.log
    }

    /// Looks up a key in the state machine and returns its value, if present.
    pub fn kv_get(&self, key: &[u8]) -> Option<&[u8]> {
        self.store.get(key).map(|v| v.as_slice())
    }

    /// Returns the total size of the cluster (peers + self).
    pub(crate) fn cluster_size(&self) -> usize {
        1 + self.peers.len()
    }

    /// Checks if a given number constitutes a majority of the cluster.
    pub(crate) fn is_majority(&self, n: usize) -> bool {
        n * 2 > self.cluster_size()
    }

    /// Creates a snapshot of the current state for persistence checks.
    pub(crate) fn cookie(&self) -> PersistCookie {
        PersistCookie {
            term: self.current_term,
            voted_for: self.voted_for,
            last_index: self.log.last_index(),
            last_term: self.log.last_term(),
        }
    }

    /// Allocates a new unique `IoId` for tracking an I/O operation.
    pub(crate) fn alloc_io(&mut self) -> IoId {
        let id = IoId {
            incarnation: self.incarnation,
            local: self.next_local,
        };
        self.next_local = self.next_local.saturating_add(1);
        id
    }

    /// Checks if a given NodeId is part of the cluster.
    pub(crate) fn is_member(&self, id: NodeId) -> bool {
        id == self.id || self.peers.contains(&id)
    }

    /// Bumps the log generation counter, used to invalidate pending I/O acks.
    pub(crate) fn bump_log_gen(&mut self) {
        self.log_gen = self.log_gen.saturating_add(1);
    }

    /// Creates an `AeAck` acknowledging an `AppendEntries` operation.
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

    /// Updates the durably recorded last index and term after a successful log sync.
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

    /// Updates the durable metadata (term and vote) after a successful sync.
    pub(crate) fn note_durable_meta(&mut self, cookie: PersistCookie) {
        if cookie.term > self.durable.term {
            self.durable.term = cookie.term;
            self.durable.voted_for = cookie.voted_for;
        } else if cookie.term == self.durable.term {
            self.durable.voted_for = cookie.voted_for;
        }
        self.note_durable_index(cookie.last_index, cookie.last_term);
    }

    /// Pushes an effect to start or reset the election timer.
    pub(crate) fn arm_election(effects: &mut Vec<Effect>) {
        effects.push(Effect::ArmTimer {
            id: TIMER_ELECTION,
            kind: TimerKind::Election,
        });
    }

    /// Pushes an effect to start or reset the heartbeat timer.
    pub(crate) fn arm_heartbeat(effects: &mut Vec<Effect>) {
        effects.push(Effect::ArmTimer {
            id: TIMER_HEARTBEAT,
            kind: TimerKind::Heartbeat,
        });
    }

    /// Pushes an effect to cancel the election timer.
    pub(crate) fn cancel_election(effects: &mut Vec<Effect>) {
        effects.push(Effect::CancelTimer { id: TIMER_ELECTION });
    }

    /// Pushes an effect to cancel the heartbeat timer.
    pub(crate) fn cancel_heartbeat(effects: &mut Vec<Effect>) {
        effects.push(Effect::CancelTimer {
            id: TIMER_HEARTBEAT,
        });
    }

    /// Steps down the node to Follower role in a new term.
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

    /// Emits a single `Fsync` effect without appending data, tracking it in flight.
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

    /// Checks if there's an in-flight `Fsync` that covers the specified log index.
    pub(crate) fn log_fsync_covers(&self, match_index: Index) -> bool {
        self.in_flight.values().any(|job| match job.kind {
            IoKind::LogFsync { ack: Some(ack) } => {
                ack.log_gen == self.log_gen && ack.success && ack.match_index >= match_index
            }
            IoKind::LogFsync { ack: None } => job.cookie.last_index >= match_index,
            _ => false,
        })
    }

    /// Drops a tracked in-flight `Fsync` operation by ID.
    pub(crate) fn drop_paired_fsync(&mut self, fsync_id: IoId) {
        self.in_flight.remove(&fsync_id);
    }
}

impl Default for Node {
    /// Implements the `Default` trait, creating a new `Node` with ID 0.
    fn default() -> Self {
        Self::new(NodeId(0), Vec::new())
    }
}
