//! `fn step(node, event) -> Vec<Effect>`. Synchronous. No `now`, no RNG.
//!
//! Spec: `docs/02-architecture.md`.

use crate::effect::Effect;
use crate::event::{Event, IoError, Message};
use crate::raft::election;
use crate::raft::replication;
use crate::raft::state::{IoKind, Node, PersistCookie, TIMER_ELECTION, TIMER_HEARTBEAT};
use crate::types::Index;
use crate::wal::record::{scan, WalRecord};

impl Node {
    pub fn step(&mut self, event: Event) -> Vec<Effect> {
        step(self, event)
    }
}

pub fn step(node: &mut Node, event: Event) -> Vec<Effect> {
    match event {
        Event::TimerFired { timer } if timer == TIMER_ELECTION => {
            election::on_election_timeout(node)
        }
        Event::TimerFired { timer } if timer == TIMER_HEARTBEAT => replication::on_heartbeat(node),
        Event::TimerFired { .. } => Vec::new(),
        Event::MessageReceived { from, msg } => {
            if !node.is_member(from) {
                return Vec::new();
            }
            match msg {
                Message::Ping => Vec::new(),
                Message::RequestVote { .. } => election::on_request_vote(node, from, msg),
                Message::RequestVoteResp { .. } => election::on_request_vote_resp(node, from, msg),
                Message::AppendEntries { .. } => replication::on_append_entries(node, from, msg),
                Message::AppendEntriesResp { .. } => {
                    replication::on_append_entries_resp(node, from, msg)
                }
            }
        }
        Event::Recover { durable } => recover(node, durable),
        Event::ClientRequest { req } => replication::client_request(node, req),
        Event::IoComplete { id, result } => io_complete(node, id, result),
    }
}

fn recover(node: &mut Node, durable: Vec<u8>) -> Vec<Effect> {
    let id = node.id;
    let peers = node.peers.clone();
    let incarnation = node.incarnation.saturating_add(1);
    let reject_ok_ae = node.reject_ok_ae;
    let skip_vote_persist = node.skip_vote_persist;
    *node = Node::new(id, peers);
    node.incarnation = incarnation;
    node.reject_ok_ae = reject_ok_ae;
    node.skip_vote_persist = skip_vote_persist;

    let (records, _) = scan(&durable);
    for record in records {
        match record {
            WalRecord::Meta { term, voted_for } => {
                node.current_term = term;
                node.voted_for = voted_for;
            }
            WalRecord::Entry(entry) => {
                node.log.append(entry);
            }
            WalRecord::Truncate { index } => {
                if node.log.truncate_after(index) {
                    node.bump_log_gen();
                }
            }
        }
    }
    node.durable = PersistCookie {
        term: node.current_term,
        voted_for: node.voted_for,
        last_index: node.log.last_index(),
        last_term: node.log.last_term(),
    };
    node.commit_index = Index(0);
    node.last_applied = Index(0);

    let mut effects = Vec::new();
    Node::arm_election(&mut effects);
    effects
}

fn io_complete(
    node: &mut Node,
    id: crate::types::IoId,
    result: Result<(), IoError>,
) -> Vec<Effect> {
    if id.incarnation != node.incarnation {
        return Vec::new();
    }
    let Some(job) = node.in_flight.remove(&id) else {
        return Vec::new();
    };
    match (job.kind, result) {
        (IoKind::AppendMeta { .. }, Ok(())) => Vec::new(),
        (IoKind::AppendMeta { fsync_id }, Err(_)) => {
            node.drop_paired_fsync(fsync_id);
            Vec::new()
        }
        (IoKind::AppendLog { .. }, Ok(())) => Vec::new(),
        (IoKind::AppendLog { from, fsync_id }, Err(_)) => {
            node.drop_paired_fsync(fsync_id);
            replication::on_append_log_err(node, from)
        }
        (IoKind::VoteFsync { work }, Ok(())) => {
            node.note_durable_meta(job.cookie);
            election::on_vote_fsync_ok(node, work, job.cookie.term)
        }
        (IoKind::VoteFsync { .. }, Err(_)) => Vec::new(),
        (IoKind::LogFsync { ack }, Ok(())) => replication::on_log_fsync_ok(node, job.cookie, ack),
        (IoKind::LogFsync { .. }, Err(_)) => {
            replication::on_log_fsync_err(node, job.cookie.last_index)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::{ClientError, ClientResp, Effect, IoOp};
    use crate::event::{ClientReq, Event, IoError, Message};
    use crate::raft::state::Role;
    use crate::types::{ClientId, Cmd, Index, NodeId, RequestId, Term};
    use crate::wal::record::{LogEntry, LogPayload};
    use std::collections::VecDeque;

    fn put(request: u64) -> Event {
        Event::ClientRequest {
            req: ClientReq {
                client: ClientId(1),
                request: RequestId(request),
                cmd: Cmd::Put {
                    key: b"k".to_vec(),
                    value: b"v".to_vec(),
                },
            },
        }
    }

    fn get(request: u64) -> Event {
        Event::ClientRequest {
            req: ClientReq {
                client: ClientId(1),
                request: RequestId(request),
                cmd: Cmd::Get { key: b"k".to_vec() },
            },
        }
    }

    fn drain_io(
        node: &mut Node,
        start: Vec<Effect>,
        append_ok: bool,
        fsync_ok: bool,
    ) -> Vec<ClientResp> {
        let mut replies = Vec::new();
        let mut q = VecDeque::from(start);
        while let Some(eff) = q.pop_front() {
            match eff {
                Effect::Reply { resp, .. } => replies.push(resp),
                Effect::IoSubmit { id, op } => {
                    let result = match op {
                        IoOp::Append { .. } => {
                            if append_ok {
                                Ok(())
                            } else {
                                Err(IoError::IoFailed)
                            }
                        }
                        IoOp::Fsync => {
                            if fsync_ok {
                                Ok(())
                            } else {
                                Err(IoError::FsyncFailed)
                            }
                        }
                    };
                    q.extend(node.step(Event::IoComplete { id, result }));
                }
                _ => {}
            }
        }
        replies
    }

    fn elect_solo(node: &mut Node) {
        let effects = node.step(Event::TimerFired {
            timer: TIMER_ELECTION,
        });
        let _ = drain_io(node, effects, true, true);
        assert_eq!(node.role(), Role::Leader);
    }

    fn solo_leader() -> Node {
        let mut node = Node::new(NodeId(0), Vec::new());
        node.step(Event::Recover { durable: vec![] });
        elect_solo(&mut node);
        node
    }

    fn drive_results(
        node: &mut Node,
        start: Event,
        append_ok: bool,
        fsync_ok: bool,
    ) -> Vec<ClientResp> {
        let effects = node.step(start);
        drain_io(node, effects, append_ok, fsync_ok)
    }

    fn drive(node: &mut Node, start: Event, fsync_ok: bool) -> Vec<ClientResp> {
        drive_results(node, start, true, fsync_ok)
    }

    fn fsync_ids(effects: &[Effect]) -> Vec<crate::types::IoId> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::IoSubmit {
                    id,
                    op: IoOp::Fsync,
                } => Some(*id),
                _ => None,
            })
            .collect()
    }

    fn append_ids(effects: &[Effect]) -> Vec<crate::types::IoId> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::IoSubmit {
                    id,
                    op: IoOp::Append { .. },
                } => Some(*id),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn put_then_fsync_ok_then_get_sees_value() {
        let mut node = solo_leader();
        let put_replies = drive(&mut node, put(1), true);
        assert_eq!(
            put_replies,
            vec![ClientResp::Ok {
                value: b"v".to_vec()
            }]
        );
        let get_replies = drive(&mut node, get(2), true);
        assert_eq!(
            get_replies,
            vec![ClientResp::Ok {
                value: b"v".to_vec()
            }]
        );
    }

    #[test]
    fn follower_client_request_is_not_leader() {
        let mut node = Node::new(NodeId(0), Vec::new());
        node.step(Event::Recover { durable: vec![] });
        let replies = drive(&mut node, put(1), true);
        assert_eq!(replies, vec![ClientResp::Err(ClientError::NotLeader)]);
    }

    #[test]
    fn fsync_err_then_covering_get_sees_put() {
        let mut node = solo_leader();
        let put_replies = drive(&mut node, put(1), false);
        assert_eq!(put_replies, vec![ClientResp::Err(ClientError::Io)]);
        let get_replies = drive(&mut node, get(2), true);
        assert_eq!(
            get_replies,
            vec![ClientResp::Ok {
                value: b"v".to_vec()
            }]
        );
        assert!(node.durable().last_index >= crate::types::Index(2));
    }

    #[test]
    fn fsync_err_then_recover_loses_put() {
        let mut node = solo_leader();
        drive(&mut node, put(1), false);
        node.step(Event::Recover { durable: vec![] });
        elect_solo(&mut node);
        let get_replies = drive(&mut node, get(2), true);
        assert_eq!(get_replies, vec![ClientResp::Err(ClientError::NotFound)]);
    }

    #[test]
    fn crash_before_fsync_recover_loses_put() {
        let mut node = solo_leader();
        let effects = node.step(put(1));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::IoSubmit {
                op: IoOp::Append { .. },
                ..
            }
        )));
        node.step(Event::Recover { durable: vec![] });
        elect_solo(&mut node);
        let get_replies = drive(&mut node, get(2), true);
        assert_eq!(get_replies, vec![ClientResp::Err(ClientError::NotFound)]);
    }

    #[test]
    fn duplicate_after_apply_replies_without_append() {
        let mut node = solo_leader();
        drive(&mut node, put(1), true);
        let effects = node.step(put(1));
        assert_eq!(effects.len(), 1);
        assert!(matches!(
            effects[0],
            Effect::Reply {
                resp: ClientResp::Ok { .. },
                ..
            }
        ));
    }

    #[test]
    fn duplicate_in_flight_replies_when_committed() {
        let mut node = solo_leader();
        let first = node.step(put(1));
        let second = node.step(put(1));
        assert!(second.is_empty());
        let mut replies = Vec::new();
        replies.extend(drain_io(&mut node, first, true, true));
        assert_eq!(replies.len(), 2);
        assert!(replies.iter().all(|r| matches!(r, ClientResp::Ok { .. })));
    }

    #[test]
    fn pipelined_fsync_reorder_does_not_regress_durable() {
        let mut node = solo_leader();
        let e1 = node.step(put(1));
        let e2 = node.step(put(2));
        for id in append_ids(&e1).into_iter().chain(append_ids(&e2)) {
            node.step(Event::IoComplete { id, result: Ok(()) });
        }
        let f2 = fsync_ids(&e2)[0];
        let f1 = fsync_ids(&e1)[0];
        node.step(Event::IoComplete {
            id: f2,
            result: Ok(()),
        });
        let after_first = node.durable().last_index;
        node.step(Event::IoComplete {
            id: f1,
            result: Ok(()),
        });
        assert_eq!(node.durable().last_index, after_first);
        assert!(node.durable().last_index >= crate::types::Index(2));
    }

    #[test]
    fn append_err_does_not_apply() {
        let mut node = solo_leader();
        let put_replies = drive_results(&mut node, put(1), false, true);
        assert_eq!(put_replies, vec![ClientResp::Err(ClientError::Io)]);
        let get_replies = drive(&mut node, get(2), true);
        assert_eq!(get_replies, vec![ClientResp::Err(ClientError::NotFound)]);
    }

    #[test]
    fn recover_rebuilds_log_without_applying() {
        let bytes =
            crate::wal::record::encode_record(&crate::wal::record::WalRecord::Entry(LogEntry {
                term: Term(1),
                payload: LogPayload::Client {
                    client: ClientId(1),
                    request: RequestId(1),
                    cmd: Cmd::Put {
                        key: b"k".to_vec(),
                        value: b"v".to_vec(),
                    },
                },
            }))
            .unwrap();
        let mut node = Node::new(NodeId(0), Vec::new());
        node.step(Event::Recover { durable: bytes });
        assert_eq!(node.commit_index(), Index(0));
        assert!(node.kv_get(b"k").is_none());
        assert_eq!(node.last_log_index(), Index(1));
        elect_solo(&mut node);
        assert_eq!(node.kv_get(b"k"), Some(b"v".as_slice()));
    }

    #[test]
    fn stale_completion_after_recover_is_ignored() {
        let mut node = solo_leader();
        let effects = node.step(put(1));
        let fsync_id = effects
            .iter()
            .find_map(|e| match e {
                Effect::IoSubmit {
                    id,
                    op: IoOp::Fsync,
                } => Some(*id),
                _ => None,
            })
            .unwrap();
        node.step(Event::Recover { durable: vec![] });
        let replies = node.step(Event::IoComplete {
            id: fsync_id,
            result: Ok(()),
        });
        assert!(replies.is_empty());
    }

    #[test]
    fn vote_fsync_err_does_not_become_leader() {
        let mut node = Node::new(NodeId(0), Vec::new());
        node.step(Event::Recover { durable: vec![] });
        let campaign = node.step(Event::TimerFired {
            timer: TIMER_ELECTION,
        });
        for id in append_ids(&campaign) {
            node.step(Event::IoComplete { id, result: Ok(()) });
        }
        for id in fsync_ids(&campaign) {
            node.step(Event::IoComplete {
                id,
                result: Err(IoError::FsyncFailed),
            });
        }
        assert_eq!(node.role(), Role::Candidate);
    }

    #[test]
    fn request_vote_not_sent_in_same_batch_as_persist() {
        let mut node = Node::new(NodeId(0), vec![NodeId(1)]);
        node.step(Event::Recover { durable: vec![] });
        let campaign = node.step(Event::TimerFired {
            timer: TIMER_ELECTION,
        });
        assert!(!campaign.iter().any(|e| matches!(e, Effect::Send { .. })));
        for id in append_ids(&campaign) {
            let more = node.step(Event::IoComplete { id, result: Ok(()) });
            assert!(!more.iter().any(|e| matches!(e, Effect::Send { .. })));
        }
        let mut sent = Vec::new();
        for id in fsync_ids(&campaign) {
            sent.extend(node.step(Event::IoComplete { id, result: Ok(()) }));
        }
        assert!(sent.iter().any(|e| matches!(
            e,
            Effect::Send {
                to: NodeId(1),
                msg: Message::RequestVote { .. },
            }
        )));
        assert_eq!(node.role(), Role::Candidate);
    }

    #[test]
    fn append_entries_success_not_sent_before_fsync() {
        let mut follower = Node::new(NodeId(1), vec![NodeId(0)]);
        follower.step(Event::Recover { durable: vec![] });
        let ae = Message::AppendEntries {
            term: Term(1),
            prev_index: Index(0),
            prev_term: Term(0),
            entries: vec![LogEntry {
                term: Term(1),
                payload: LogPayload::NoOp,
            }],
            leader_commit: Index(0),
        };
        let effects = follower.step(Event::MessageReceived {
            from: NodeId(0),
            msg: ae,
        });
        assert!(!effects.iter().any(|e| matches!(
            e,
            Effect::Send {
                msg: Message::AppendEntriesResp { success: true, .. },
                ..
            }
        )));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::IoSubmit {
                op: IoOp::Fsync,
                ..
            }
        )));
        let mut after = Vec::new();
        for id in append_ids(&effects) {
            after.extend(follower.step(Event::IoComplete { id, result: Ok(()) }));
        }
        assert!(!after.iter().any(|e| matches!(
            e,
            Effect::Send {
                msg: Message::AppendEntriesResp { success: true, .. },
                ..
            }
        )));
        for id in fsync_ids(&effects) {
            after.extend(follower.step(Event::IoComplete { id, result: Ok(()) }));
        }
        assert!(after.iter().any(|e| matches!(
            e,
            Effect::Send {
                msg: Message::AppendEntriesResp { success: true, .. },
                ..
            }
        )));
    }

    fn ae_noop(term: u64) -> Message {
        Message::AppendEntries {
            term: Term(term),
            prev_index: Index(0),
            prev_term: Term(0),
            entries: vec![LogEntry {
                term: Term(term),
                payload: LogPayload::NoOp,
            }],
            leader_commit: Index(0),
        }
    }

    fn has_ae_success(effects: &[Effect]) -> bool {
        effects.iter().any(|e| {
            matches!(
                e,
                Effect::Send {
                    msg: Message::AppendEntriesResp { success: true, .. },
                    ..
                }
            )
        })
    }

    #[test]
    fn buggify_reject_ok_ae_nacks_matching_append() {
        let mut follower = Node::new(NodeId(1), vec![NodeId(0)]);
        follower.set_reject_ok_ae(true);
        follower.step(Event::Recover { durable: vec![] });
        let effects = follower.step(Event::MessageReceived {
            from: NodeId(0),
            msg: ae_noop(1),
        });
        assert!(!has_ae_success(&effects));
        let mut after = Vec::new();
        for id in append_ids(&effects) {
            after.extend(follower.step(Event::IoComplete { id, result: Ok(()) }));
        }
        for id in fsync_ids(&effects) {
            after.extend(follower.step(Event::IoComplete { id, result: Ok(()) }));
        }
        assert!(after.iter().any(|e| matches!(
            e,
            Effect::Send {
                msg: Message::AppendEntriesResp { success: false, .. },
                ..
            }
        )));
        assert_eq!(follower.last_log_index(), Index(0));
    }

    #[test]
    fn duplicate_ae_does_not_ack_until_durable() {
        let mut follower = Node::new(NodeId(1), vec![NodeId(0)]);
        follower.step(Event::Recover { durable: vec![] });
        let first = follower.step(Event::MessageReceived {
            from: NodeId(0),
            msg: ae_noop(1),
        });
        assert!(!has_ae_success(&first));
        let retry = follower.step(Event::MessageReceived {
            from: NodeId(0),
            msg: ae_noop(1),
        });
        assert!(!has_ae_success(&retry));
        assert!(!retry.iter().any(|e| matches!(
            e,
            Effect::IoSubmit {
                op: IoOp::Append { .. },
                ..
            }
        )));
        let mut after = Vec::new();
        for id in append_ids(&first) {
            after.extend(follower.step(Event::IoComplete { id, result: Ok(()) }));
        }
        for id in fsync_ids(&first) {
            after.extend(follower.step(Event::IoComplete { id, result: Ok(()) }));
        }
        assert!(has_ae_success(&after));
        assert_eq!(follower.durable().last_index, Index(1));
    }

    #[test]
    fn stale_log_fsync_after_truncate_does_not_ack_old_prefix() {
        let mut follower = Node::new(NodeId(1), vec![NodeId(0)]);
        follower.step(Event::Recover { durable: vec![] });
        let first = follower.step(Event::MessageReceived {
            from: NodeId(0),
            msg: ae_noop(1),
        });
        let second = follower.step(Event::MessageReceived {
            from: NodeId(0),
            msg: ae_noop(2),
        });
        assert!(!has_ae_success(&second));
        for id in append_ids(&first) {
            follower.step(Event::IoComplete { id, result: Ok(()) });
        }
        let mut stale = Vec::new();
        for id in fsync_ids(&first) {
            stale.extend(follower.step(Event::IoComplete { id, result: Ok(()) }));
        }
        assert!(!has_ae_success(&stale));
        let mut live = Vec::new();
        for id in append_ids(&second) {
            live.extend(follower.step(Event::IoComplete { id, result: Ok(()) }));
        }
        for id in fsync_ids(&second) {
            live.extend(follower.step(Event::IoComplete { id, result: Ok(()) }));
        }
        assert!(has_ae_success(&live));
        assert_eq!(follower.log().term_at(Index(1)), Some(Term(2)));
        assert_eq!(follower.last_log_index(), Index(1));
    }

    #[test]
    fn recover_applies_truncate_and_does_not_keep_discarded_suffix() {
        let put =
            crate::wal::record::encode_record(&crate::wal::record::WalRecord::Entry(LogEntry {
                term: Term(1),
                payload: LogPayload::Client {
                    client: ClientId(1),
                    request: RequestId(1),
                    cmd: Cmd::Put {
                        key: b"k".to_vec(),
                        value: b"old".to_vec(),
                    },
                },
            }))
            .unwrap();
        let cut = crate::wal::record::encode_record(&crate::wal::record::WalRecord::Truncate {
            index: Index(0),
        })
        .unwrap();
        let noop =
            crate::wal::record::encode_record(&crate::wal::record::WalRecord::Entry(LogEntry {
                term: Term(2),
                payload: LogPayload::NoOp,
            }))
            .unwrap();
        let mut durable = put;
        durable.extend_from_slice(&cut);
        durable.extend_from_slice(&noop);
        let mut node = Node::new(NodeId(0), Vec::new());
        node.step(Event::Recover { durable });
        assert_eq!(node.last_log_index(), Index(1));
        assert_eq!(node.log().term_at(Index(1)), Some(Term(2)));
        assert!(node.kv_get(b"k").is_none());
    }

    #[test]
    fn non_member_rpc_is_ignored() {
        let mut node = Node::new(NodeId(0), vec![NodeId(1)]);
        node.step(Event::Recover { durable: vec![] });
        let effects = node.step(Event::MessageReceived {
            from: NodeId(9),
            msg: Message::RequestVote {
                term: Term(1),
                last_log_index: Index(0),
                last_log_term: Term(0),
            },
        });
        assert!(effects.is_empty());
        assert_eq!(node.current_term(), Term(0));
    }

    #[test]
    fn non_member_vote_does_not_win_election() {
        let mut node = Node::new(NodeId(0), vec![NodeId(1), NodeId(2)]);
        node.step(Event::Recover { durable: vec![] });
        let campaign = node.step(Event::TimerFired {
            timer: TIMER_ELECTION,
        });
        let _ = drain_io(&mut node, campaign, true, true);
        assert_eq!(node.role(), Role::Candidate);
        let effects = node.step(Event::MessageReceived {
            from: NodeId(9),
            msg: Message::RequestVoteResp {
                term: node.current_term(),
                granted: true,
            },
        });
        assert!(effects.is_empty());
        assert_eq!(node.role(), Role::Candidate);
    }
}
