//! Candidate/follower/leader transitions, votes, election timeouts as `ArmTimer`.
//!
//! Persist-before-send: no `RequestVote` / grant until the covering Meta Fsync Ok.
//! Spec: `docs/02-architecture.md`.

use crate::effect::Effect;
use crate::event::Message;
use crate::raft::replication;
use crate::raft::state::{IoKind, Node, PersistSpec, Role, VoteWork};
use crate::types::{Index, NodeId, Term};
use crate::wal::record::{LogEntry, LogPayload};

pub(crate) fn on_election_timeout(node: &mut Node) -> Vec<Effect> {
    if node.role == Role::Leader {
        return Vec::new();
    }
    node.current_term = Term(node.current_term.0.saturating_add(1));
    node.voted_for = Some(node.id);
    node.role = Role::Candidate;
    node.votes.clear();
    let mut effects = Vec::new();
    node.persist(
        PersistSpec {
            meta: true,
            truncate_to: None,
            log_from: None,
            fsync: IoKind::VoteFsync {
                work: VoteWork::Campaign,
            },
        },
        &mut effects,
    );
    Node::arm_election(&mut effects);
    if node.skip_vote_persist {
        effects.extend(on_vote_fsync_ok(
            node,
            VoteWork::Campaign,
            node.current_term,
        ));
    }
    effects
}

pub(crate) fn on_vote_fsync_ok(node: &mut Node, work: VoteWork, cookie_term: Term) -> Vec<Effect> {
    match work {
        VoteWork::Campaign => {
            if node.role != Role::Candidate || node.current_term != cookie_term {
                return Vec::new();
            }
            let mut effects = Vec::new();
            let last_log_index = node.log.last_index();
            let last_log_term = node.log.last_term();
            let msg = Message::RequestVote {
                term: node.current_term,
                last_log_index,
                last_log_term,
            };
            for &peer in &node.peers {
                effects.push(Effect::Send {
                    to: peer,
                    msg: msg.clone(),
                });
            }
            node.votes.insert(node.id);
            effects.extend(maybe_become_leader(node));
            effects
        }
        VoteWork::Reply { to, granted } => {
            if node.current_term != cookie_term {
                return Vec::new();
            }
            vec![Effect::Send {
                to,
                msg: Message::RequestVoteResp {
                    term: node.current_term,
                    granted,
                },
            }]
        }
        VoteWork::Silent => Vec::new(),
    }
}

pub(crate) fn on_request_vote(node: &mut Node, from: NodeId, msg: Message) -> Vec<Effect> {
    let Message::RequestVote {
        term,
        last_log_index,
        last_log_term,
    } = msg
    else {
        return Vec::new();
    };
    let mut effects = Vec::new();
    if term < node.current_term {
        effects.push(Effect::Send {
            to: from,
            msg: Message::RequestVoteResp {
                term: node.current_term,
                granted: false,
            },
        });
        return effects;
    }
    let mut persist_meta = false;
    if term > node.current_term {
        node.step_down_to(term, &mut effects);
        persist_meta = true;
    }
    let log_ok = last_log_term > node.log.last_term()
        || (last_log_term == node.log.last_term() && last_log_index >= node.log.last_index());
    let can_grant = log_ok && (node.voted_for.is_none() || node.voted_for == Some(from));
    if can_grant {
        node.voted_for = Some(from);
        node.persist(
            PersistSpec {
                meta: true,
                truncate_to: None,
                log_from: None,
                fsync: IoKind::VoteFsync {
                    work: VoteWork::Reply {
                        to: from,
                        granted: true,
                    },
                },
            },
            &mut effects,
        );
        Node::arm_election(&mut effects);
        if node.skip_vote_persist {
            effects.extend(on_vote_fsync_ok(
                node,
                VoteWork::Reply {
                    to: from,
                    granted: true,
                },
                node.current_term,
            ));
        }
    } else if persist_meta {
        node.persist(
            PersistSpec {
                meta: true,
                truncate_to: None,
                log_from: None,
                fsync: IoKind::VoteFsync {
                    work: VoteWork::Reply {
                        to: from,
                        granted: false,
                    },
                },
            },
            &mut effects,
        );
    } else {
        effects.push(Effect::Send {
            to: from,
            msg: Message::RequestVoteResp {
                term: node.current_term,
                granted: false,
            },
        });
    }
    effects
}

pub(crate) fn on_request_vote_resp(node: &mut Node, from: NodeId, msg: Message) -> Vec<Effect> {
    let Message::RequestVoteResp { term, granted } = msg else {
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
    if term != node.current_term || node.role != Role::Candidate || !granted {
        return effects;
    }
    node.votes.insert(from);
    effects.extend(maybe_become_leader(node));
    effects
}

fn maybe_become_leader(node: &mut Node) -> Vec<Effect> {
    if node.role != Role::Candidate || !node.is_majority(node.votes.len()) {
        return Vec::new();
    }
    node.role = Role::Leader;
    node.votes.clear();
    node.match_index_self = node.durable.last_index;
    node.next_index.clear();
    node.match_index.clear();
    let next = Index(node.log.last_index().0.saturating_add(1));
    for &peer in &node.peers {
        node.next_index.insert(peer, next);
        node.match_index.insert(peer, Index(0));
    }
    let mut effects = Vec::new();
    Node::cancel_election(&mut effects);
    node.log.append(LogEntry {
        term: node.current_term,
        payload: LogPayload::NoOp,
    });
    let from = node.log.last_index();
    node.persist(
        PersistSpec {
            meta: false,
            truncate_to: None,
            log_from: Some(from),
            fsync: IoKind::LogFsync { ack: None },
        },
        &mut effects,
    );
    replication::replicate_all(node, &mut effects);
    Node::arm_heartbeat(&mut effects);
    effects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::{Effect, IoOp};
    use crate::event::{Event, Message};
    use crate::raft::state::{Role, TIMER_ELECTION, TIMER_HEARTBEAT};
    use crate::types::NodeId;

    fn solo() -> Node {
        let mut node = Node::new(NodeId(0), Vec::new());
        node.step(Event::Recover { durable: vec![] });
        node
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

    fn has_send(effects: &[Effect]) -> bool {
        effects.iter().any(|e| matches!(e, Effect::Send { .. }))
    }

    #[test]
    fn election_timeout_does_not_send_before_fsync() {
        let mut node = solo();
        let effects = node.step(Event::TimerFired {
            timer: TIMER_ELECTION,
        });
        assert!(!has_send(&effects));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::IoSubmit {
                op: IoOp::Fsync,
                ..
            }
        )));
        assert_eq!(node.role(), Role::Candidate);
    }

    #[test]
    fn skip_vote_persist_sends_request_vote_in_persist_batch() {
        let mut node = Node::new(NodeId(0), vec![NodeId(1)]);
        node.step(Event::Recover { durable: vec![] });
        node.set_skip_vote_persist(true);
        let effects = node.step(Event::TimerFired {
            timer: TIMER_ELECTION,
        });
        assert!(has_send(&effects));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::IoSubmit {
                op: IoOp::Fsync,
                ..
            }
        )));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Send {
                msg: Message::RequestVote { .. },
                ..
            }
        )));
    }

    #[test]
    fn fsync_ok_then_solo_becomes_leader() {
        let mut node = solo();
        let campaign = node.step(Event::TimerFired {
            timer: TIMER_ELECTION,
        });
        assert!(!has_send(&campaign));
        for e in &campaign {
            if let Effect::IoSubmit {
                id,
                op: IoOp::Append { .. },
            } = e
            {
                node.step(Event::IoComplete {
                    id: *id,
                    result: Ok(()),
                });
            }
        }
        let mut after = Vec::new();
        for id in fsync_ids(&campaign) {
            after.extend(node.step(Event::IoComplete { id, result: Ok(()) }));
        }
        assert_eq!(node.role(), Role::Leader);
        assert!(after.iter().any(|e| matches!(
            e,
            Effect::ArmTimer {
                id: TIMER_HEARTBEAT,
                ..
            }
        )));
    }
}
