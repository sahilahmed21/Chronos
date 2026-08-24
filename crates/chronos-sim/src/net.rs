//! Simulated network: delay, loss, duplicate, partition. `MsgId` assigned at enqueue.
//!
//! Always assigns `MsgId`, including drops. Dup is a second delivery.
//! Spec: `docs/02-architecture.md` § Network.

use std::collections::BTreeMap;

use chronos_protocol::{Message, MsgId, NodeId, Timestamp};

use crate::scheduler::DropReason;

struct InFlight {
    from: NodeId,
    to: NodeId,
    msg: Message,
}

pub struct Delivery {
    pub msg_id: MsgId,
    pub from: NodeId,
    pub to: NodeId,
    pub msg: Message,
    pub delivery_at: Timestamp,
}

pub enum SendOutcome {
    Deliver(Delivery),
    Dropped {
        msg_id: MsgId,
        from: NodeId,
        to: NodeId,
        reason: DropReason,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpcKind {
    Ping,
    RequestVote,
    RequestVoteResp,
    AppendEntries,
    AppendEntriesResp,
}

pub fn rpc_kind(msg: &Message) -> RpcKind {
    match msg {
        Message::Ping => RpcKind::Ping,
        Message::RequestVote { .. } => RpcKind::RequestVote,
        Message::RequestVoteResp { .. } => RpcKind::RequestVoteResp,
        Message::AppendEntries { .. } => RpcKind::AppendEntries,
        Message::AppendEntriesResp { .. } => RpcKind::AppendEntriesResp,
    }
}

/// Drop matching outbound RPCs at send time. `None` fields are wildcards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropRule {
    pub from: Option<NodeId>,
    pub to: Option<NodeId>,
    pub kind: Option<RpcKind>,
}

impl DropRule {
    pub fn matches(&self, from: NodeId, to: NodeId, msg: &Message) -> bool {
        if self.from.is_some_and(|f| f != from) {
            return false;
        }
        if self.to.is_some_and(|t| t != to) {
            return false;
        }
        if self.kind.is_some_and(|k| k != rpc_kind(msg)) {
            return false;
        }
        true
    }
}

pub struct SimNet {
    connected: Vec<Vec<bool>>,
    inflight: BTreeMap<MsgId, InFlight>,
    next_msg: u64,
}

impl SimNet {
    pub fn new(n: u8) -> Self {
        let dim = usize::from(n);
        Self {
            connected: vec![vec![true; dim]; dim],
            inflight: BTreeMap::new(),
            next_msg: 0,
        }
    }

    pub fn connected(&self, from: NodeId, to: NodeId) -> bool {
        let (i, j) = (usize::from(from.0), usize::from(to.0));
        self.connected
            .get(i)
            .and_then(|row| row.get(j))
            .copied()
            .unwrap_or(false)
    }

    pub fn set_connected(&mut self, from: NodeId, to: NodeId, yes: bool, asymmetric: bool) {
        self.set_dir(from, to, yes);
        if !asymmetric {
            self.set_dir(to, from, yes);
        }
    }

    fn set_dir(&mut self, from: NodeId, to: NodeId, yes: bool) {
        let (i, j) = (usize::from(from.0), usize::from(to.0));
        if let Some(cell) = self.connected.get_mut(i).and_then(|row| row.get_mut(j)) {
            *cell = yes;
        }
    }

    fn alloc_msg(&mut self) -> MsgId {
        let id = MsgId(self.next_msg);
        self.next_msg = self.next_msg.saturating_add(1);
        id
    }

    pub fn send(
        &mut self,
        from: NodeId,
        to: NodeId,
        msg: Message,
        now: Timestamp,
        delay_ns: u64,
        loss: bool,
    ) -> SendOutcome {
        let msg_id = self.alloc_msg();
        if !self.connected(from, to) {
            return SendOutcome::Dropped {
                msg_id,
                from,
                to,
                reason: DropReason::Partition,
            };
        }
        if loss {
            return SendOutcome::Dropped {
                msg_id,
                from,
                to,
                reason: DropReason::Loss,
            };
        }
        self.inflight.insert(
            msg_id,
            InFlight {
                from,
                to,
                msg: msg.clone(),
            },
        );
        SendOutcome::Deliver(Delivery {
            msg_id,
            from,
            to,
            msg,
            delivery_at: Timestamp(now.0.saturating_add(delay_ns)),
        })
    }

    /// Second copy of a send that already delivered. Does not re-roll loss.
    pub fn send_duplicate(
        &mut self,
        from: NodeId,
        to: NodeId,
        msg: Message,
        now: Timestamp,
        delay_ns: u64,
    ) -> SendOutcome {
        self.send(from, to, msg, now, delay_ns, false)
    }

    pub fn take(&mut self, msg_id: MsgId) -> Option<(NodeId, NodeId, Message)> {
        self.inflight.remove(&msg_id).map(|m| (m.from, m.to, m.msg))
    }
}

#[cfg(test)]
mod tests {
    use super::{SendOutcome, SimNet};
    use crate::scheduler::DropReason;
    use chronos_protocol::{Message, MsgId, NodeId, Timestamp};

    #[test]
    fn send_assigns_msgid() {
        let mut net = SimNet::new(2);
        match net.send(NodeId(0), NodeId(1), Message::Ping, Timestamp(0), 0, false) {
            SendOutcome::Deliver(d) => {
                assert_eq!(d.msg_id, MsgId(0));
                assert_eq!(d.to, NodeId(1));
                assert_eq!(d.delivery_at, Timestamp(0));
            }
            SendOutcome::Dropped { .. } => panic!("expected deliver, got drop"),
        }
    }

    #[test]
    fn disconnected_drops_but_assigns_msgid() {
        let mut net = SimNet::new(2);
        net.set_connected(NodeId(0), NodeId(1), false, false);
        match net.send(NodeId(0), NodeId(1), Message::Ping, Timestamp(0), 0, false) {
            SendOutcome::Dropped {
                msg_id,
                reason: DropReason::Partition,
                ..
            } => assert_eq!(msg_id, MsgId(0)),
            SendOutcome::Deliver(_) => panic!("expected drop"),
            SendOutcome::Dropped { reason, .. } => panic!("wrong reason {reason:?}"),
        }
    }

    #[test]
    fn loss_assigns_msgid() {
        let mut net = SimNet::new(2);
        match net.send(NodeId(0), NodeId(1), Message::Ping, Timestamp(0), 0, true) {
            SendOutcome::Dropped {
                msg_id,
                reason: DropReason::Loss,
                ..
            } => assert_eq!(msg_id, MsgId(0)),
            _ => panic!("expected loss"),
        }
    }

    #[test]
    fn asymmetric_partition_one_direction() {
        let mut net = SimNet::new(2);
        net.set_connected(NodeId(0), NodeId(1), false, true);
        assert!(!net.connected(NodeId(0), NodeId(1)));
        assert!(net.connected(NodeId(1), NodeId(0)));
    }
}
