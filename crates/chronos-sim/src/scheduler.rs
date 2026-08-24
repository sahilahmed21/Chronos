//! Min-heap of world events. Key is `(Timestamp, Seq)` with one global `Seq` at enqueue.
//!
//! Spec: `docs/02-architecture.md` D4. Jitter scheduled times; do not shuffle a ready list.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use chronos_protocol::{ClientReq, IoError, IoId, Message, MsgId, NodeId, TimerId, Timestamp};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldEvent {
    TimerFired {
        node: NodeId,
        timer: TimerId,
        generation: u64,
    },
    MessageDeliver {
        from: NodeId,
        to: NodeId,
        msg_id: MsgId,
        msg: Message,
    },
    IoComplete {
        node: NodeId,
        id: IoId,
        result: Result<(), IoError>,
        sync_len: Option<usize>,
        life: u64,
    },
    Crash {
        node: NodeId,
        /// `None` = decide from disk tail + RNG when the crash is applied.
        torn_extra: Option<u64>,
    },
    Partition {
        from: NodeId,
        to: NodeId,
        connected: bool,
        asymmetric: bool,
    },
    ClientInject {
        node: NodeId,
        req: ClientReq,
    },
    Recover {
        node: NodeId,
    },
    Dropped {
        from: NodeId,
        to: NodeId,
        msg_id: MsgId,
        reason: DropReason,
    },
    FailNextFsync {
        node: NodeId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropReason {
    Loss,
    Partition,
}

struct HeapItem {
    time: Timestamp,
    seq: u64,
    event: WorldEvent,
}

impl PartialEq for HeapItem {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time && self.seq == other.seq
    }
}

impl Eq for HeapItem {}

impl Ord for HeapItem {
    fn cmp(&self, other: &Self) -> Ordering {
        other.time.cmp(&self.time).then(other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct Scheduler {
    heap: BinaryHeap<HeapItem>,
    seq: u64,
    now: Timestamp,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            seq: 0,
            now: Timestamp(0),
        }
    }

    pub fn now(&self) -> Timestamp {
        self.now
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    pub fn peek_time(&self) -> Option<Timestamp> {
        self.heap.peek().map(|item| item.time)
    }

    pub fn peek_event(&self) -> Option<&WorldEvent> {
        self.heap.peek().map(|item| &item.event)
    }

    pub fn has_non_timer(&self) -> bool {
        self.heap
            .iter()
            .any(|item| !matches!(item.event, WorldEvent::TimerFired { .. }))
    }

    /// `seq` is one counter for the run. Never reset. Never per-node.
    /// Times before `now` clamp to `now` so virtual time never moves backward.
    pub fn enqueue(&mut self, time: Timestamp, event: WorldEvent) {
        let time = Timestamp(time.0.max(self.now.0));
        let seq = self.seq;
        self.seq = self.seq.saturating_add(1);
        self.heap.push(HeapItem { time, seq, event });
    }

    pub fn pop(&mut self) -> Option<(Timestamp, u64, WorldEvent)> {
        let item = self.heap.pop()?;
        if item.time > self.now {
            self.now = item.time;
        }
        Some((item.time, item.seq, item.event))
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Scheduler, WorldEvent};
    use chronos_protocol::{NodeId, Timestamp};

    #[test]
    fn equal_time_ordered_by_enqueue_seq() {
        let mut s = Scheduler::new();
        s.enqueue(
            Timestamp(10),
            WorldEvent::Crash {
                node: NodeId(0),
                torn_extra: None,
            },
        );
        s.enqueue(
            Timestamp(10),
            WorldEvent::Crash {
                node: NodeId(1),
                torn_extra: None,
            },
        );
        let (t0, seq0, a) = s.pop().unwrap();
        let (t1, seq1, b) = s.pop().unwrap();
        assert_eq!(t0, Timestamp(10));
        assert_eq!(t1, Timestamp(10));
        assert_eq!(seq0, 0);
        assert_eq!(seq1, 1);
        match a {
            WorldEvent::Crash { node, .. } => assert_eq!(node, NodeId(0)),
            other => panic!("expected A, got {other:?}"),
        }
        match b {
            WorldEvent::Crash { node, .. } => assert_eq!(node, NodeId(1)),
            other => panic!("expected B, got {other:?}"),
        }
        assert!(s.pop().is_none());
        assert_eq!(s.now(), Timestamp(10));
    }

    #[test]
    fn enqueue_in_the_past_does_not_rewind_now() {
        let mut s = Scheduler::new();
        s.enqueue(
            Timestamp(10),
            WorldEvent::Crash {
                node: NodeId(0),
                torn_extra: None,
            },
        );
        s.pop();
        assert_eq!(s.now(), Timestamp(10));
        s.enqueue(
            Timestamp(5),
            WorldEvent::Crash {
                node: NodeId(1),
                torn_extra: None,
            },
        );
        let (t, _, _) = s.pop().unwrap();
        assert_eq!(t, Timestamp(10));
        assert_eq!(s.now(), Timestamp(10));
    }
}
