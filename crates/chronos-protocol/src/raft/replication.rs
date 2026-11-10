//! AppendEntries, nextIndex, matchIndex, current-term commit (Figure 8), no-op on leadership.
//!
//! Persist-before-ack: successful AE Send only after covering log Fsync Ok.
//! Spec: `docs/02-architecture.md`.

use crate::effect::{ClientError, ClientResp, Effect};
use crate::event::{ClientReq, Message};
use crate::kv::machine::apply;
use crate::raft::state::{AeAck, IoKind, Mutant, Node, Pending, PersistSpec, Role, VoteWork};
use crate::types::{Cmd, Index, NodeId, Term};
use crate::wal::record::{encode_record, LogEntry, LogPayload, WalRecord};

/// Sends an `AppendEntries` RPC to all peer nodes to replicate log entries.
///
/// This is typically called by a leader to push new entries or to send a heartbeat.
pub(crate) fn replicate_all(node: &Node, effects: &mut Vec<Effect>) {
    for &peer in &node.peers {
        effects.push(Effect::Send {
            to: peer,
            msg: append_entries_for(node, peer),
        });
    }
}

/// Constructs an `AppendEntries` message tailored for a specific peer.
///
/// Looks up the peer's `next_index` to determine which log entries need to be sent,
/// along with the appropriate `prev_index` and `prev_term` for consistency checks.
fn append_entries_for(node: &Node, peer: NodeId) -> Message {
    let next = node.next_index.get(&peer).copied().unwrap_or(Index(1));
    let prev_index = Index(next.0.saturating_sub(1));
    let prev_term = node.log.term_at(prev_index).unwrap_or(Term(0));
    Message::AppendEntries {
        term: node.current_term,
        prev_index,
        prev_term,
        entries: node.log.suffix_from(next),
        leader_commit: node.commit_index,
    }
}

/// Handles the heartbeat timer firing.
///
/// If the node is a leader, it sends `AppendEntries` (empty or with data) to all peers
/// and rearms the heartbeat timer.
pub(crate) fn on_heartbeat(node: &mut Node) -> Vec<Effect> {
    if node.role != Role::Leader {
        return Vec::new();
    }
    let mut effects = Vec::new();
    replicate_all(node, &mut effects);
    Node::arm_heartbeat(&mut effects);
    effects
}

/// Handles a successful `fsync` of the log.
///
/// Updates the node's durable state based on the provided cookie. If the node is a leader,
/// it checks if it can commit any new entries and replies to clients. If the sync was
/// for a follower's `AppendEntries` processing (indicated by `ack`), it sends the reply to the leader.
pub(crate) fn on_log_fsync_ok(
    node: &mut Node,
    cookie: crate::raft::state::PersistCookie,
    ack: Option<AeAck>,
) -> Vec<Effect> {
    if let Some(ack) = &ack {
        if ack.log_gen != node.log_gen {
            return Vec::new();
        }
        if ack.success && node.log.last_index() < ack.match_index {
            return Vec::new();
        }
    }
    node.note_durable_meta(cookie);
    let mut effects = Vec::new();
    if node.role == Role::Leader {
        node.match_index_self = node.durable.last_index;
        maybe_commit(node, &mut effects);
    }
    if let Some(ack) = ack {
        effects.push(Effect::Send {
            to: ack.to,
            msg: Message::AppendEntriesResp {
                term: node.current_term,
                success: ack.success,
                match_index: ack.match_index,
            },
        });
        if ack.success {
            advance_follower_commit(
                node,
                ack.leader_commit
                    .min(node.durable.last_index)
                    .min(ack.match_index),
            );
            apply_committed(node, &mut effects);
        }
    }
    effects
}

/// Handles an error during log `fsync`.
///
/// Iterates over pending client requests up to `last_index` and replies with an I/O error
/// to inform clients that their requests could not be durably recorded.
pub(crate) fn on_log_fsync_err(node: &mut Node, last_index: Index) -> Vec<Effect> {
    let mut effects = Vec::new();
    for p in &mut node.pending {
        if p.index <= last_index && !p.replied {
            let n = 1 + p.extra_replies;
            p.replied = true;
            p.extra_replies = 0;
            effects.extend(reply_n(
                p.client,
                p.request,
                ClientResp::Err(ClientError::Io),
                n,
            ));
        }
    }
    effects
}

/// Handles a failure to append an entry to the log file.
///
/// If the failed append was beyond the durable index, it truncates the in-memory log back
/// to a safe state, increments the log generation, and replies with I/O errors to affected clients.
pub(crate) fn on_append_log_err(node: &mut Node, from: Index) -> Vec<Effect> {
    if from > node.durable.last_index && node.log.truncate_after(Index(from.0.saturating_sub(1))) {
        node.bump_log_gen();
    }
    let mut effects = Vec::new();
    let mut i = 0;
    while i < node.pending.len() {
        if node.pending[i].index >= from {
            let p = node.pending.remove(i);
            if !p.replied {
                effects.extend(reply_n(
                    p.client,
                    p.request,
                    ClientResp::Err(ClientError::Io),
                    1 + p.extra_replies,
                ));
            }
        } else {
            i += 1;
        }
    }
    effects
}

/// Handles an incoming `AppendEntries` RPC.
///
/// Performs term and log-matching checks. If successful, it truncates any conflicting log
/// entries, appends new ones, and schedules an `fsync` before replying to the leader.
pub(crate) fn on_append_entries(node: &mut Node, from: NodeId, msg: Message) -> Vec<Effect> {
    let Message::AppendEntries {
        term,
        prev_index,
        prev_term,
        entries,
        leader_commit,
    } = msg
    else {
        return Vec::new();
    };
    let mut effects = Vec::new();
    if term < node.current_term {
        effects.push(Effect::Send {
            to: from,
            msg: Message::AppendEntriesResp {
                term: node.current_term,
                success: false,
                match_index: Index(0),
            },
        });
        return effects;
    }
    let mut persist_meta = false;
    if term > node.current_term || node.role != Role::Follower {
        if term > node.current_term {
            node.step_down_to(term, &mut effects);
            persist_meta = true;
        } else {
            node.role = Role::Follower;
            node.votes.clear();
            node.next_index.clear();
            node.match_index.clear();
            Node::cancel_heartbeat(&mut effects);
            Node::arm_election(&mut effects);
        }
    } else {
        Node::arm_election(&mut effects);
    }

    if !node.log.matches(prev_index, prev_term) {
        let ack = node.ae_ack(from, false, Index(0), leader_commit);
        if persist_meta {
            node.persist(
                PersistSpec {
                    meta: true,
                    truncate_to: None,
                    log_from: None,
                    fsync: IoKind::LogFsync { ack: Some(ack) },
                },
                &mut effects,
            );
        } else {
            effects.push(Effect::Send {
                to: from,
                msg: Message::AppendEntriesResp {
                    term: node.current_term,
                    success: false,
                    match_index: Index(0),
                },
            });
        }
        return effects;
    }

    if node.reject_ok_ae {
        let ack = node.ae_ack(from, false, Index(0), leader_commit);
        if persist_meta {
            node.persist(
                PersistSpec {
                    meta: true,
                    truncate_to: None,
                    log_from: None,
                    fsync: IoKind::LogFsync { ack: Some(ack) },
                },
                &mut effects,
            );
        } else {
            effects.push(Effect::Send {
                to: from,
                msg: Message::AppendEntriesResp {
                    term: node.current_term,
                    success: false,
                    match_index: Index(0),
                },
            });
        }
        return effects;
    }

    let saved_log = node.log.clone();
    let saved_gen = node.log_gen;
    let mut idx = prev_index.0.saturating_add(1);
    let mut first_appended = None;
    let mut truncate_to = None;
    let mut appended = false;
    for entry in entries {
        if let Some(existing) = node.log.term_at(Index(idx)) {
            if existing != entry.term {
                if node.log.truncate_after(Index(idx.saturating_sub(1))) {
                    node.bump_log_gen();
                    truncate_to = Some(Index(idx.saturating_sub(1)));
                }
                node.log.append(entry);
                appended = true;
                if first_appended.is_none() {
                    first_appended = Some(Index(idx));
                }
            }
        } else {
            node.log.append(entry);
            appended = true;
            if first_appended.is_none() {
                first_appended = Some(Index(idx));
            }
        }
        idx = idx.saturating_add(1);
    }
    let ack_index = Index(idx.saturating_sub(1));
    let ack = node.ae_ack(from, true, ack_index, leader_commit);

    if !appended && !persist_meta {
        if ack_index <= node.durable.last_index {
            advance_follower_commit(
                node,
                leader_commit.min(node.durable.last_index).min(ack_index),
            );
            apply_committed(node, &mut effects);
            effects.push(Effect::Send {
                to: from,
                msg: Message::AppendEntriesResp {
                    term: node.current_term,
                    success: true,
                    match_index: ack_index,
                },
            });
        } else if node.log_fsync_covers(ack_index) {
            // Covering LogFsync already in flight; do not ack until it completes.
        } else {
            node.persist_fsync_only(IoKind::LogFsync { ack: Some(ack) }, &mut effects);
        }
        return effects;
    }

    if !node.persist(
        PersistSpec {
            meta: persist_meta,
            truncate_to,
            log_from: first_appended,
            fsync: IoKind::LogFsync { ack: Some(ack) },
        },
        &mut effects,
    ) {
        node.log = saved_log;
        node.log_gen = saved_gen;
        effects.push(Effect::Send {
            to: from,
            msg: Message::AppendEntriesResp {
                term: node.current_term,
                success: false,
                match_index: Index(0),
            },
        });
    } else if node.is_mutant(Mutant::AckBeforeFsync) {
        effects.push(Effect::Send {
            to: from,
            msg: Message::AppendEntriesResp {
                term: node.current_term,
                success: true,
                match_index: ack_index,
            },
        });
    }
    effects
}

/// Handles an incoming `AppendEntriesResp` (reply) from a peer.
///
/// If successful, the leader updates its `match_index` and `next_index` for the peer,
/// checks if it can advance its commit index, and sends further entries if needed.
/// If unsuccessful, it decrements `next_index` and retries.
pub(crate) fn on_append_entries_resp(node: &mut Node, from: NodeId, msg: Message) -> Vec<Effect> {
    let Message::AppendEntriesResp {
        term,
        success,
        match_index,
    } = msg
    else {
        return Vec::new();
    };
    let mut effects = Vec::new();
    if term > node.current_term {
        node.step_down_to(term, &mut effects);
        node.persist(
            PersistSpec {
                meta: true,
                truncate_to: None,
                log_from: None,
                fsync: IoKind::VoteFsync {
                    work: VoteWork::Silent,
                },
            },
            &mut effects,
        );
        return effects;
    }
    if term != node.current_term || node.role != Role::Leader {
        return effects;
    }
    if success {
        let prev = node.match_index.get(&from).copied().unwrap_or(Index(0));
        if match_index > prev {
            node.match_index.insert(from, match_index);
        }
        node.next_index
            .insert(from, Index(match_index.0.saturating_add(1)));
        maybe_commit(node, &mut effects);
        if match_index < node.log.last_index() {
            effects.push(Effect::Send {
                to: from,
                msg: append_entries_for(node, from),
            });
        }
    } else {
        let next = node.next_index.get(&from).copied().unwrap_or(Index(1));
        let dec = Index(next.0.saturating_sub(1).max(1));
        node.next_index.insert(from, dec);
        effects.push(Effect::Send {
            to: from,
            msg: append_entries_for(node, from),
        });
    }
    effects
}

/// Processes a new client request.
///
/// If the node is not a leader, it rejects the request. Otherwise, it checks for idempotency,
/// appends the request to its log, and initiates an `fsync` and replication to peers.
pub(crate) fn client_request(node: &mut Node, req: ClientReq) -> Vec<Effect> {
    if node.role != Role::Leader {
        return vec![Effect::Reply {
            to: req.client,
            request: req.request,
            resp: ClientResp::Err(ClientError::NotLeader),
        }];
    }
    if node.is_mutant(Mutant::GetFromMemory) {
        if let Cmd::Get { key } = &req.cmd {
            let resp = match node.store.get(key) {
                Some(value) => ClientResp::Ok {
                    value: value.clone(),
                },
                None => ClientResp::Err(ClientError::NotFound),
            };
            return vec![Effect::Reply {
                to: req.client,
                request: req.request,
                resp,
            }];
        }
    }
    if let Some(resp) = node.idempotency.get(&(req.client, req.request)) {
        return vec![Effect::Reply {
            to: req.client,
            request: req.request,
            resp: resp.clone(),
        }];
    }
    if let Some(pending) = node
        .pending
        .iter_mut()
        .find(|p| p.client == req.client && p.request == req.request)
    {
        if pending.replied {
            return vec![Effect::Reply {
                to: req.client,
                request: req.request,
                resp: ClientResp::Err(ClientError::Io),
            }];
        }
        pending.extra_replies = pending.extra_replies.saturating_add(1);
        return Vec::new();
    }
    if let Some(index) = node.log.find_client(req.client, req.request) {
        node.pending.push(Pending {
            index,
            client: req.client,
            request: req.request,
            replied: false,
            extra_replies: 0,
        });
        return Vec::new();
    }

    let entry = LogEntry {
        term: node.current_term,
        payload: LogPayload::Client {
            client: req.client,
            request: req.request,
            cmd: req.cmd.clone(),
        },
    };
    let Some(_) = encode_record(&WalRecord::Entry(entry.clone())) else {
        return vec![Effect::Reply {
            to: req.client,
            request: req.request,
            resp: ClientResp::Err(ClientError::Invalid),
        }];
    };
    let index = node.log.append(entry);
    node.pending.push(Pending {
        index,
        client: req.client,
        request: req.request,
        replied: false,
        extra_replies: 0,
    });
    let mut effects = Vec::new();
    node.persist(
        PersistSpec {
            meta: false,
            truncate_to: None,
            log_from: Some(index),
            fsync: IoKind::LogFsync { ack: None },
        },
        &mut effects,
    );
    replicate_all(node, &mut effects);
    effects
}

/// Attempts to advance the leader's commit index.
///
/// Iterates over uncommitted log entries in the current term to find the highest index
/// that has been replicated to a majority of nodes. If found, it updates the commit index
/// and triggers application to the state machine.
fn maybe_commit(node: &mut Node, effects: &mut Vec<Effect>) {
    if node.role != Role::Leader {
        return;
    }
    let mut best = node.commit_index;
    for n in node.commit_index.0.saturating_add(1)..=node.log.last_index().0 {
        let idx = Index(n);
        if node.log.term_at(idx) != Some(node.current_term)
            && !node.is_mutant(Mutant::CommitOldTerm)
        {
            continue;
        }
        if majority_at(node, idx) {
            best = idx;
        }
    }
    if best > node.commit_index {
        node.commit_index = best;
        apply_committed(node, effects);
        replicate_all(node, effects);
    }
}

/// Checks if the given index is replicated on a majority of cluster nodes.
fn majority_at(node: &Node, index: Index) -> bool {
    let mut n = 0;
    if node.match_index_self >= index {
        n += 1;
    }
    for &peer in &node.peers {
        if node.match_index.get(&peer).copied().unwrap_or(Index(0)) >= index {
            n += 1;
        }
    }
    node.is_majority(n)
}

/// Safely advances a follower's commit index based on the leader's commit index.
///
/// The follower's commit index is bounded by what it has durably persisted and
/// the last index it has appended to its log.
fn advance_follower_commit(node: &mut Node, up_to: Index) {
    let cap = up_to
        .min(node.durable.last_index)
        .min(node.log.last_index());
    if cap > node.commit_index {
        node.commit_index = cap;
    }
}

/// Applies all newly committed log entries to the key-value state machine.
///
/// For client requests, it delegates to the KV `apply` logic. If the node is the leader,
/// it also generates success replies for the clients.
fn apply_committed(node: &mut Node, effects: &mut Vec<Effect>) {
    while node.last_applied < node.commit_index {
        node.last_applied = Index(node.last_applied.0.saturating_add(1));
        let Some(entry) = node.log.entry(node.last_applied).cloned() else {
            break;
        };
        match entry.payload {
            LogPayload::NoOp => {}
            LogPayload::Client {
                client,
                request,
                cmd,
            } => {
                let mut dropped = std::collections::BTreeMap::new();
                let table = if node.is_mutant(Mutant::DropIdempotency) {
                    &mut dropped
                } else {
                    &mut node.idempotency
                };
                let resp = apply(&mut node.store, table, client, request, &cmd);
                let Some(pos) = node
                    .pending
                    .iter()
                    .position(|p| p.index == node.last_applied)
                else {
                    continue;
                };
                let p = node.pending.remove(pos);
                if node.role != Role::Leader {
                    continue;
                }
                if !p.replied {
                    effects.extend(reply_n(p.client, p.request, resp, 1 + p.extra_replies));
                } else if p.extra_replies > 0 {
                    effects.extend(reply_n(p.client, p.request, resp, p.extra_replies));
                }
            }
        }
    }
}

/// Generates `n` copies of a client reply effect.
///
/// Used when a client retries a request multiple times while it is still pending;
/// this ensures all pending request IDs get a response.
pub(crate) fn reply_n(
    to: crate::types::ClientId,
    request: crate::types::RequestId,
    resp: ClientResp,
    n: u32,
) -> Vec<Effect> {
    (0..n)
        .map(|_| Effect::Reply {
            to,
            request,
            resp: resp.clone(),
        })
        .collect()
}
