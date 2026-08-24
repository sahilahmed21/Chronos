//! N nodes, dispatch `Event` to `step`, interpret `Effect`s in emission order.
//!
//! P1 `drive()` stays delay-0 VecDeque. P2 `Cluster` owns the `(time, seq)` heap.
//! Spec: `docs/02-architecture.md` § Simulated world.

use std::collections::{BTreeMap, VecDeque};

use chronos_protocol::{
    ClientId, ClientReq, ClientResp, Cmd, Effect, Event, IoError, IoId, IoOp, Message, MsgId, Node,
    NodeId, RequestId, Role, Term, TimerId, TimerKind, Timestamp,
};

use crate::check::{majority_fully_connected, CheckFail, CheckView, Checker, NodeSnap};
use crate::disk::SimDisk;
use crate::fuzz::FaultConfig;
use crate::history::History;
use crate::net::{Delivery, DropRule, SendOutcome, SimNet};
use crate::rng::Rng;
use crate::scheduler::{DropReason, Scheduler, WorldEvent};
use crate::trace::Trace;

/// Run `event` and every delay-0 completion until the disk queue is idle.
/// Replies are collected in the order `step` emitted them.
pub fn drive(node: &mut Node, disk: &mut SimDisk, event: Event) -> Vec<(ClientId, ClientResp)> {
    let mut replies = Vec::new();
    let mut q = VecDeque::new();
    q.push_back(event);
    while let Some(ev) = q.pop_front() {
        let effects = node.step(ev);
        disk.submit(&effects);
        for effect in &effects {
            if let Effect::Reply { to, resp, .. } = effect {
                replies.push((*to, resp.clone()));
            }
        }
        while let Some(complete) = disk.pop() {
            q.push_back(complete);
        }
    }
    replies
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimConfig {
    pub n: u8,
    pub election_min_ns: u64,
    pub election_max_ns: u64,
    pub heartbeat_ns: u64,
    pub io_delay_min_ns: u64,
    pub io_delay_max_ns: u64,
    pub net_delay_min_ns: u64,
    pub net_delay_max_ns: u64,
    pub jitter_max_ns: u64,
    pub max_ns: u64,
    pub drop_ppm: u32,
    pub dup_ppm: u32,
    pub torn_suffix: bool,
    pub fsync_ok_but_not_durable: bool,
    pub buggify_slow_fsync: bool,
    pub buggify_fsync_extra_ns: u64,
    pub buggify_election_edge_max: bool,
    pub buggify_reject_ok_ae: bool,
    pub record_roles: bool,
    pub check_safety: bool,
    pub check_engineering: bool,
    pub check_liveness: bool,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            n: 3,
            election_min_ns: 150_000_000,
            election_max_ns: 300_000_000,
            heartbeat_ns: 50_000_000,
            io_delay_min_ns: 0,
            io_delay_max_ns: 0,
            net_delay_min_ns: 0,
            net_delay_max_ns: 0,
            jitter_max_ns: 0,
            max_ns: 10_000_000_000,
            drop_ppm: 0,
            dup_ppm: 0,
            torn_suffix: true,
            fsync_ok_but_not_durable: false,
            buggify_slow_fsync: false,
            buggify_fsync_extra_ns: 0,
            buggify_election_edge_max: false,
            buggify_reject_ok_ae: false,
            record_roles: false,
            check_safety: true,
            check_engineering: true,
            check_liveness: false,
        }
    }
}

impl SimConfig {
    pub fn faults(&self) -> FaultConfig {
        FaultConfig {
            drop_ppm: self.drop_ppm,
            dup_ppm: self.dup_ppm,
            torn_suffix: self.torn_suffix,
            fsync_ok_but_not_durable: self.fsync_ok_but_not_durable,
            buggify_slow_fsync: self.buggify_slow_fsync,
            buggify_fsync_extra_ns: self.buggify_fsync_extra_ns,
            buggify_election_edge_max: self.buggify_election_edge_max,
            buggify_reject_ok_ae: self.buggify_reject_ok_ae,
        }
    }
}

/// Content-addressed send identity. Not `MsgId` (that is an enqueue ordinal).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SendKey {
    pub from: NodeId,
    pub to: NodeId,
    pub hash: [u8; 32],
}

/// One recorded loss or duplicate of an encoded RPC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeliveryToken {
    pub from: NodeId,
    pub to: NodeId,
    pub hash: [u8; 32],
}

/// Delays captured from a probabilistic drain. Bound to the *full* recorded
/// run only (`DelayBind::Recorded`). Subset DD uses `DelayBind::Defaults`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayBook {
    pub send_delay: BTreeMap<SendKey, VecDeque<u64>>,
    pub io_delay: BTreeMap<(u8, u64, u8), VecDeque<u64>>,
    pub election: BTreeMap<(u8, u64), VecDeque<u64>>,
}

/// Whether schedule replay may consume `ReplayBook` FIFOs.
///
/// `Recorded` is only legal for the unfiltered schedule (same sends as the
/// observe run). Subsets use `Defaults` so they cannot steal another send's delay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelayBind {
    Recorded,
    Defaults,
}

#[derive(Clone, Debug)]
enum DrainPolicy {
    Swarm,
    Schedule {
        bind_delays: bool,
        book: ReplayBook,
        drops: Vec<DeliveryToken>,
        dups: Vec<DeliveryToken>,
    },
}

/// Extra choices observed during a swarm drain (torn, drops, dups, delays).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObservedSchedule {
    pub crash_torn: BTreeMap<(u64, u8), u64>,
    pub drops: Vec<DeliveryToken>,
    pub dups: Vec<DeliveryToken>,
    pub book: ReplayBook,
}

fn message_hash(msg: &Message) -> [u8; 32] {
    let bytes = msg.encode().unwrap_or_else(|| vec![255]);
    crate::trace::sha256(&bytes)
}

fn send_key(from: NodeId, to: NodeId, msg: &Message) -> SendKey {
    SendKey {
        from,
        to,
        hash: message_hash(msg),
    }
}

fn delivery_token(from: NodeId, to: NodeId, msg: &Message) -> DeliveryToken {
    DeliveryToken {
        from,
        to,
        hash: message_hash(msg),
    }
}

fn queue_push<K: Ord>(map: &mut BTreeMap<K, VecDeque<u64>>, key: K, delay: u64) {
    map.entry(key).or_default().push_back(delay);
}

fn queue_pop<K: Copy + Ord>(map: &mut BTreeMap<K, VecDeque<u64>>, key: K) -> Option<u64> {
    let delay = map.get_mut(&key)?.pop_front();
    if map.get(&key).is_some_and(|q| q.is_empty()) {
        map.remove(&key);
    }
    delay
}

fn take_token(tokens: &mut Vec<DeliveryToken>, token: DeliveryToken) -> bool {
    if let Some(i) = tokens
        .iter()
        .position(|t| t.from == token.from && t.to == token.to && t.hash == token.hash)
    {
        tokens.remove(i);
        true
    } else {
        false
    }
}

pub struct Cluster {
    cfg: SimConfig,
    rng: Rng,
    sched: Scheduler,
    nodes: Vec<Node>,
    disks: Vec<SimDisk>,
    net: SimNet,
    trace: Trace,
    timer_gen: BTreeMap<(NodeId, TimerId), u64>,
    life: Vec<u64>,
    alive: Vec<bool>,
    delivered: Vec<(NodeId, NodeId, MsgId)>,
    messages: Vec<(NodeId, NodeId, Message)>,
    dropped: Vec<(NodeId, NodeId, MsgId, DropReason)>,
    replies: Vec<(ClientId, ClientResp)>,
    timers_stepped: Vec<(NodeId, TimerId)>,
    role_log: Vec<(Timestamp, NodeId, Role, Term)>,
    drop_rules: Vec<DropRule>,
    applied_io: Vec<(NodeId, IoId, Result<(), IoError>)>,
    checker: Checker,
    history: History,
    check_fail: Option<CheckFail>,
    skip_vote_persist: Vec<bool>,
    last_fault_at: Timestamp,
    event_seq: u64,
    dup_sends: u64,
    torn_applied: bool,
    policy: DrainPolicy,
    observed: ObservedSchedule,
}

impl Cluster {
    pub fn new(seed: u64, cfg: SimConfig) -> Self {
        let n = usize::from(cfg.n);
        let mut cluster = Self {
            net: SimNet::new(cfg.n),
            rng: Rng::new(seed),
            sched: Scheduler::new(),
            nodes: (0..cfg.n).map(|i| make_node(NodeId(i), cfg.n)).collect(),
            disks: (0..n).map(|_| SimDisk::new()).collect(),
            trace: Trace::new(),
            timer_gen: BTreeMap::new(),
            life: vec![0; n],
            alive: vec![true; n],
            delivered: Vec::new(),
            messages: Vec::new(),
            dropped: Vec::new(),
            replies: Vec::new(),
            timers_stepped: Vec::new(),
            role_log: Vec::new(),
            drop_rules: Vec::new(),
            applied_io: Vec::new(),
            checker: Checker::new(),
            history: History::new(),
            check_fail: None,
            skip_vote_persist: vec![false; n],
            last_fault_at: Timestamp(0),
            event_seq: 0,
            dup_sends: 0,
            torn_applied: false,
            policy: DrainPolicy::Swarm,
            observed: ObservedSchedule::default(),
            cfg,
        };
        for disk in &mut cluster.disks {
            disk.fsync_ok_but_not_durable = cluster.cfg.fsync_ok_but_not_durable;
        }
        for i in 0..n {
            cluster.nodes[i].set_reject_ok_ae(cluster.cfg.buggify_reject_ok_ae);
        }
        cluster
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.idx(id).map(|i| &self.nodes[i])
    }

    pub fn node_count(&self) -> u8 {
        self.cfg.n
    }

    pub fn delivered(&self) -> &[(NodeId, NodeId, MsgId)] {
        &self.delivered
    }

    pub fn messages(&self) -> &[(NodeId, NodeId, Message)] {
        &self.messages
    }

    pub fn dropped(&self) -> &[(NodeId, NodeId, MsgId, DropReason)] {
        &self.dropped
    }

    pub fn dup_sends(&self) -> u64 {
        self.dup_sends
    }

    pub fn torn_applied(&self) -> bool {
        self.torn_applied
    }

    pub fn observed(&self) -> &ObservedSchedule {
        &self.observed
    }

    /// Schedule replay: ppm unused, world `Rng` unread.
    /// `DelayBind::Recorded` may pop `book` FIFOs; `Defaults` never does.
    pub fn use_schedule_replay(
        &mut self,
        book: ReplayBook,
        drops: Vec<DeliveryToken>,
        dups: Vec<DeliveryToken>,
        bind: DelayBind,
    ) {
        self.policy = DrainPolicy::Schedule {
            bind_delays: matches!(bind, DelayBind::Recorded),
            book,
            drops,
            dups,
        };
    }

    pub fn role_log(&self) -> &[(Timestamp, NodeId, Role, Term)] {
        &self.role_log
    }

    pub fn now(&self) -> Timestamp {
        self.sched.now()
    }

    pub fn peek_time(&self) -> Option<Timestamp> {
        self.sched.peek_time()
    }

    pub fn connected(&self, from: NodeId, to: NodeId) -> bool {
        self.net.connected(from, to)
    }

    pub fn alive(&self, id: NodeId) -> bool {
        self.idx(id).is_some_and(|i| self.alive[i])
    }

    pub fn add_drop_rule(&mut self, rule: DropRule) {
        self.drop_rules.push(rule);
    }

    pub fn election_safety_ok(&self) -> bool {
        self.checker.election_safety_ok()
    }

    pub fn check_fail(&self) -> Option<&CheckFail> {
        self.check_fail.as_ref()
    }

    pub fn set_skip_vote_persist(&mut self, node: NodeId, yes: bool) {
        let Some(i) = self.idx(node) else {
            return;
        };
        self.skip_vote_persist[i] = yes;
        self.nodes[i].set_skip_vote_persist(yes);
    }

    pub fn applied_io(&self) -> &[(NodeId, IoId, Result<(), IoError>)] {
        &self.applied_io
    }

    /// Pop and apply one heap event. Returns false if empty, past `max_ns`, or a check failed.
    pub fn step_once(&mut self) -> bool {
        if self.check_fail.is_some() {
            return false;
        }
        let Some(t) = self.sched.peek_time() else {
            return false;
        };
        if t.0 > self.cfg.max_ns {
            return false;
        }
        let Some((time, seq, event)) = self.sched.pop() else {
            return false;
        };
        self.apply_world(time, seq, event);
        self.check_fail.is_none()
    }

    pub fn inject_at(&mut self, time: Timestamp, event: WorldEvent) {
        self.sched.enqueue(time, event);
    }

    pub fn fail_next_fsync(&mut self, node: NodeId) {
        if self.idx(node).is_some() {
            self.sched
                .enqueue(self.sched.now(), WorldEvent::FailNextFsync { node });
        }
    }

    pub fn disk_bytes(&self, node: NodeId) -> Option<&[u8]> {
        self.idx(node).map(|i| self.disks[i].bytes.as_slice())
    }

    pub fn replies(&self) -> &[(ClientId, ClientResp)] {
        &self.replies
    }

    pub fn timers_stepped(&self) -> &[(NodeId, TimerId)] {
        &self.timers_stepped
    }

    pub fn digest(&self) -> [u8; 32] {
        self.trace.digest()
    }

    pub fn encoded_trace(&self) -> Vec<u8> {
        self.trace.concat()
    }

    pub fn durable_len(&self, node: NodeId) -> Option<usize> {
        self.idx(node).map(|i| self.disks[i].durable_len)
    }

    pub fn inject_recover(&mut self, node: NodeId) {
        if self.idx(node).is_none() {
            return;
        }
        self.sched
            .enqueue(self.sched.now(), WorldEvent::Recover { node });
    }

    pub fn inject_client(&mut self, node: NodeId, req: ClientReq) {
        if self.idx(node).is_none() {
            return;
        }
        self.sched
            .enqueue(self.sched.now(), WorldEvent::ClientInject { node, req });
    }

    pub fn inject_send(&mut self, from: NodeId, to: NodeId, msg: Message) {
        if self.idx(from).is_none() || self.idx(to).is_none() {
            return;
        }
        self.observe_outgoing(from, to, &msg);
        self.dispatch_send(from, to, msg);
    }

    pub fn inject_timer(&mut self, node: NodeId, timer: TimerId) {
        if self.idx(node).is_none() {
            return;
        }
        let generation = self.bump_timer(node, timer);
        let at = Timestamp(self.sched.now().0.saturating_add(self.jitter()));
        self.sched.enqueue(
            at,
            WorldEvent::TimerFired {
                node,
                timer,
                generation,
            },
        );
    }

    pub fn inject_crash(&mut self, node: NodeId) {
        if self.idx(node).is_none() {
            return;
        }
        self.sched.enqueue(
            self.sched.now(),
            WorldEvent::Crash {
                node,
                torn_extra: None,
            },
        );
    }

    pub fn inject_partition(&mut self, from: NodeId, to: NodeId, connected: bool) {
        self.inject_partition_ex(from, to, connected, false);
    }

    pub fn inject_partition_ex(
        &mut self,
        from: NodeId,
        to: NodeId,
        connected: bool,
        asymmetric: bool,
    ) {
        if self.idx(from).is_none() || self.idx(to).is_none() {
            return;
        }
        self.sched.enqueue(
            self.sched.now(),
            WorldEvent::Partition {
                from,
                to,
                connected,
                asymmetric,
            },
        );
    }

    pub fn inject_arm_timer(&mut self, node: NodeId, timer: TimerId, kind: TimerKind) {
        if self.idx(node).is_none() {
            return;
        }
        self.arm_timer(node, timer, kind);
    }

    pub fn inject_cancel_timer(&mut self, node: NodeId, timer: TimerId) {
        if self.idx(node).is_none() {
            return;
        }
        self.bump_timer(node, timer);
    }

    pub fn drain(&mut self) {
        loop {
            if self.has_live_leader() && !self.sched.has_non_timer() {
                break;
            }
            if !self.step_once() {
                break;
            }
        }
        self.finish_checks();
    }

    /// Run until `max_ns` (or a check fail). Used by swarm/minify so extras
    /// behind heartbeats still fire.
    pub fn drain_horizon(&mut self) {
        while self.step_once() {}
        self.finish_checks();
    }

    fn has_live_leader(&self) -> bool {
        self.nodes.iter().enumerate().any(|(i, n)| {
            self.alive.get(i).copied().unwrap_or(false) && n.role() == Role::Leader
        })
    }

    fn on_schedule(&self) -> bool {
        matches!(self.policy, DrainPolicy::Schedule { .. })
    }

    fn election_default(&self) -> u64 {
        if self.cfg.buggify_election_edge_max {
            self.cfg.election_max_ns
        } else {
            self.cfg.election_min_ns
        }
    }

    fn jitter(&mut self) -> u64 {
        if self.on_schedule() {
            return 0;
        }
        if self.cfg.jitter_max_ns == 0 {
            0
        } else {
            self.rng.delay_ns(0, self.cfg.jitter_max_ns)
        }
    }

    fn idx(&self, id: NodeId) -> Option<usize> {
        let i = usize::from(id.0);
        if i < self.nodes.len() {
            Some(i)
        } else {
            None
        }
    }

    fn bump_timer(&mut self, node: NodeId, timer: TimerId) -> u64 {
        let g = self.timer_gen.entry((node, timer)).or_insert(0);
        *g = g.saturating_add(1);
        *g
    }

    fn timer_is_current(&self, node: NodeId, timer: TimerId, generation: u64) -> bool {
        self.timer_gen.get(&(node, timer)).copied().unwrap_or(0) == generation
    }

    fn arm_timer(&mut self, from: NodeId, id: TimerId, kind: TimerKind) {
        let generation = self.bump_timer(from, id);
        let now = self.sched.now();
        let life = self.idx(from).map(|i| self.life[i]).unwrap_or(0);
        let dur = match kind {
            TimerKind::Election => self.election_delay(from, life),
            TimerKind::Heartbeat => self.cfg.heartbeat_ns,
        };
        let jit = match kind {
            TimerKind::Election => 0,
            TimerKind::Heartbeat => self.jitter(),
        };
        let at = Timestamp(now.0.saturating_add(dur).saturating_add(jit));
        self.sched.enqueue(
            at,
            WorldEvent::TimerFired {
                node: from,
                timer: id,
                generation,
            },
        );
    }

    fn election_delay(&mut self, node: NodeId, life: u64) -> u64 {
        let key = (node.0, life);
        let fallback = self.election_default();
        match &mut self.policy {
            DrainPolicy::Schedule {
                bind_delays, book, ..
            } => {
                if *bind_delays {
                    queue_pop(&mut book.election, key).unwrap_or(fallback)
                } else {
                    fallback
                }
            }
            DrainPolicy::Swarm => {
                let dur = if self.cfg.buggify_election_edge_max {
                    self.cfg.election_max_ns
                } else {
                    self.rng
                        .delay_ns(self.cfg.election_min_ns, self.cfg.election_max_ns)
                };
                let total = dur.saturating_add(self.jitter());
                queue_push(&mut self.observed.book.election, key, total);
                total
            }
        }
    }

    fn io_delay(&mut self, node: NodeId, op: &IoOp) -> u64 {
        let life = self.idx(node).map(|i| self.life[i]).unwrap_or(0);
        let tag = u8::from(matches!(op, IoOp::Fsync));
        let key = (node.0, life, tag);
        let fallback = self.cfg.io_delay_min_ns;
        match &mut self.policy {
            DrainPolicy::Schedule {
                bind_delays, book, ..
            } => {
                if *bind_delays {
                    queue_pop(&mut book.io_delay, key).unwrap_or(fallback)
                } else {
                    fallback
                }
            }
            DrainPolicy::Swarm => {
                let mut delay = self
                    .rng
                    .delay_ns(self.cfg.io_delay_min_ns, self.cfg.io_delay_max_ns);
                if matches!(op, IoOp::Fsync) && self.cfg.buggify_slow_fsync {
                    delay = delay.saturating_add(self.cfg.buggify_fsync_extra_ns);
                }
                let total = delay.saturating_add(self.jitter());
                queue_push(&mut self.observed.book.io_delay, key, total);
                total
            }
        }
    }

    fn bump_all_timers(&mut self, node: NodeId) {
        let timers: Vec<TimerId> = self
            .timer_gen
            .keys()
            .filter_map(|(n, t)| (*n == node).then_some(*t))
            .collect();
        for timer in timers {
            self.bump_timer(node, timer);
        }
    }

    fn reset_node(&mut self, i: usize, id: NodeId) {
        self.nodes[i] = make_node(id, self.cfg.n);
        self.nodes[i].set_reject_ok_ae(self.cfg.buggify_reject_ok_ae);
        self.nodes[i].set_skip_vote_persist(self.skip_vote_persist[i]);
    }

    fn is_up(&self, node: NodeId) -> bool {
        self.idx(node).is_some_and(|i| self.alive[i])
    }

    fn note_role(&mut self, node: NodeId) {
        if !self.cfg.record_roles {
            return;
        }
        let Some(n) = self.node(node) else {
            return;
        };
        let role = n.role();
        let term = n.current_term();
        let t = self.sched.now();
        self.role_log.push((t, node, role, term));
    }

    fn note_check(&mut self, r: Result<(), CheckFail>) {
        if self.check_fail.is_none() {
            if let Err(mut e) = r {
                if e.snapshots.is_empty() {
                    e.snapshots = self.node_snapshots();
                }
                self.check_fail = Some(e);
            }
        }
    }

    fn node_snapshots(&self) -> Vec<NodeSnap> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(i, n)| NodeSnap {
                id: n.id(),
                alive: self.alive.get(i).copied().unwrap_or(false),
                life: self.life.get(i).copied().unwrap_or(0),
                role: n.role(),
                term: n.current_term(),
                commit: n.commit_index(),
                last_applied: n.last_applied(),
                last_index: n.last_log_index(),
                durable: n.durable(),
            })
            .collect()
    }

    fn observe_outgoing(&mut self, from: NodeId, to: NodeId, msg: &Message) {
        if !self.cfg.check_safety || !self.cfg.check_engineering {
            return;
        }
        let Some(i) = self.idx(from) else {
            return;
        };
        let durable = self.nodes[i].durable();
        let r = self.checker.observe_send(from, to, msg, durable);
        self.note_check(r);
    }

    fn run_checks(&mut self) {
        if !self.cfg.check_safety || self.check_fail.is_some() {
            return;
        }
        let engineering = self.cfg.check_engineering;
        let now = self.sched.now();
        let result = self.checker.after_event(
            CheckView {
                now,
                nodes: &self.nodes,
                alive: &self.alive,
                life: &self.life,
            },
            engineering,
        );
        self.note_check(result);
    }

    fn finish_checks(&mut self) {
        if self.cfg.check_safety && self.check_fail.is_none() {
            let r = self.history.linearizable();
            self.note_check(r);
        }
        if self.cfg.check_liveness && self.check_fail.is_none() {
            let now = self.sched.now();
            let last = self.last_fault_at;
            let election_max = self.cfg.election_max_ns;
            let n = self.cfg.n;
            let majority =
                majority_fully_connected(n, &self.alive, |a, b| self.net.connected(a, b));
            let result = self.checker.check_liveness(
                now,
                last,
                election_max,
                CheckView {
                    now,
                    nodes: &self.nodes,
                    alive: &self.alive,
                    life: &self.life,
                },
                majority,
            );
            self.note_check(result);
        }
    }

    fn enqueue_delivery(&mut self, d: Delivery) {
        let at = Timestamp(d.delivery_at.0.saturating_add(self.jitter()));
        self.sched.enqueue(
            at,
            WorldEvent::MessageDeliver {
                from: d.from,
                to: d.to,
                msg_id: d.msg_id,
                msg: d.msg,
            },
        );
    }

    fn record_drop(&mut self, from: NodeId, to: NodeId, msg_id: MsgId, reason: DropReason) {
        self.dropped.push((from, to, msg_id, reason));
        self.sched.enqueue(
            self.sched.now(),
            WorldEvent::Dropped {
                from,
                to,
                msg_id,
                reason,
            },
        );
    }

    fn take_schedule_drop(&mut self, token: DeliveryToken) -> bool {
        match &mut self.policy {
            DrainPolicy::Schedule { drops, .. } => take_token(drops, token),
            DrainPolicy::Swarm => false,
        }
    }

    fn take_schedule_dup(&mut self, token: DeliveryToken) -> bool {
        match &mut self.policy {
            DrainPolicy::Schedule { dups, .. } => take_token(dups, token),
            DrainPolicy::Swarm => false,
        }
    }

    fn pop_send_delay(&mut self, key: SendKey, bind: bool, fallback: u64) -> u64 {
        if !bind {
            return fallback;
        }
        match &mut self.policy {
            DrainPolicy::Schedule { book, .. } => {
                queue_pop(&mut book.send_delay, key).unwrap_or(fallback)
            }
            DrainPolicy::Swarm => fallback,
        }
    }

    fn dispatch_send(&mut self, from: NodeId, to: NodeId, msg: Message) {
        if self.on_schedule() {
            self.dispatch_send_schedule(from, to, msg);
        } else {
            self.dispatch_send_swarm(from, to, msg);
        }
    }

    fn dispatch_send_schedule(&mut self, from: NodeId, to: NodeId, msg: Message) {
        let now = self.sched.now();
        let key = send_key(from, to, &msg);
        let token = delivery_token(from, to, &msg);
        let targeted = self
            .drop_rules
            .iter()
            .any(|rule| rule.matches(from, to, &msg));
        let min_delay = self.cfg.net_delay_min_ns;
        let bind = matches!(
            self.policy,
            DrainPolicy::Schedule {
                bind_delays: true,
                ..
            }
        );
        let loss = targeted || self.take_schedule_drop(token);
        let delay = if loss {
            0
        } else {
            self.pop_send_delay(key, bind, min_delay)
        };
        match self.net.send(from, to, msg.clone(), now, delay, loss) {
            SendOutcome::Deliver(d) => {
                self.enqueue_delivery(d);
                if self.take_schedule_dup(token) {
                    self.dup_sends = self.dup_sends.saturating_add(1);
                    let delay2 = self.pop_send_delay(key, bind, min_delay);
                    match self.net.send_duplicate(from, to, msg, now, delay2) {
                        SendOutcome::Deliver(d2) => self.enqueue_delivery(d2),
                        SendOutcome::Dropped {
                            msg_id,
                            from,
                            to,
                            reason,
                        } => self.record_drop(from, to, msg_id, reason),
                    }
                }
            }
            SendOutcome::Dropped {
                msg_id,
                from,
                to,
                reason,
            } => self.record_drop(from, to, msg_id, reason),
        }
    }

    fn dispatch_send_swarm(&mut self, from: NodeId, to: NodeId, msg: Message) {
        let now = self.sched.now();
        let key = send_key(from, to, &msg);
        let token = delivery_token(from, to, &msg);
        let targeted = self
            .drop_rules
            .iter()
            .any(|rule| rule.matches(from, to, &msg));
        let rolled_loss = !targeted && self.cfg.drop_ppm != 0 && self.rng.bool(self.cfg.drop_ppm);
        let loss = targeted || rolled_loss;
        let delay = self
            .rng
            .delay_ns(self.cfg.net_delay_min_ns, self.cfg.net_delay_max_ns);
        match self.net.send(from, to, msg.clone(), now, delay, loss) {
            SendOutcome::Deliver(d) => {
                let jit = self.jitter();
                queue_push(
                    &mut self.observed.book.send_delay,
                    key,
                    delay.saturating_add(jit),
                );
                let at = Timestamp(d.delivery_at.0.saturating_add(jit));
                self.sched.enqueue(
                    at,
                    WorldEvent::MessageDeliver {
                        from: d.from,
                        to: d.to,
                        msg_id: d.msg_id,
                        msg: d.msg,
                    },
                );
                if self.cfg.dup_ppm != 0 && self.rng.bool(self.cfg.dup_ppm) {
                    self.dup_sends = self.dup_sends.saturating_add(1);
                    self.observed.dups.push(token);
                    let delay2 = self
                        .rng
                        .delay_ns(self.cfg.net_delay_min_ns, self.cfg.net_delay_max_ns);
                    let jit2 = self.jitter();
                    queue_push(
                        &mut self.observed.book.send_delay,
                        key,
                        delay2.saturating_add(jit2),
                    );
                    match self.net.send_duplicate(from, to, msg, now, delay2) {
                        SendOutcome::Deliver(d2) => {
                            let at2 = Timestamp(d2.delivery_at.0.saturating_add(jit2));
                            self.sched.enqueue(
                                at2,
                                WorldEvent::MessageDeliver {
                                    from: d2.from,
                                    to: d2.to,
                                    msg_id: d2.msg_id,
                                    msg: d2.msg,
                                },
                            );
                        }
                        SendOutcome::Dropped {
                            msg_id,
                            from,
                            to,
                            reason,
                        } => self.record_drop(from, to, msg_id, reason),
                    }
                }
            }
            SendOutcome::Dropped {
                msg_id,
                from,
                to,
                reason,
            } => {
                if rolled_loss {
                    self.observed.drops.push(token);
                }
                self.record_drop(from, to, msg_id, reason);
            }
        }
    }

    fn resolve_torn_extra(&mut self, node: NodeId, specified: Option<u64>) -> u64 {
        let extra = if let Some(extra) = specified {
            extra
        } else if self.on_schedule() {
            0
        } else {
            let Some(i) = self.idx(node) else {
                return 0;
            };
            if !self.cfg.torn_suffix {
                return 0;
            }
            let tail = self.disks[i]
                .bytes
                .len()
                .saturating_sub(self.disks[i].durable_len) as u64;
            if tail == 0 {
                0
            } else {
                self.rng.delay_ns(0, tail)
            }
        };
        if extra > 0 {
            self.torn_applied = true;
        }
        extra
    }

    fn is_ghost(&self, event: &WorldEvent) -> bool {
        match event {
            WorldEvent::TimerFired {
                node,
                timer,
                generation,
            } => !self.is_up(*node) || !self.timer_is_current(*node, *timer, *generation),
            WorldEvent::MessageDeliver { to, .. } => !self.is_up(*to),
            WorldEvent::IoComplete { node, life, .. } => !self
                .idx(*node)
                .is_some_and(|i| self.life[i] == *life && self.alive[i]),
            WorldEvent::ClientInject { node, .. } => !self.is_up(*node),
            WorldEvent::Crash { node, .. } => !self.is_up(*node),
            WorldEvent::Recover { node } => self
                .idx(*node)
                .is_some_and(|i| self.alive[i] && self.life[i] > 0),
            _ => false,
        }
    }

    fn dismiss_ghost(&mut self, event: &WorldEvent) {
        if let WorldEvent::MessageDeliver { msg_id, .. } = event {
            self.net.take(*msg_id);
        }
    }

    fn apply_world(&mut self, time: Timestamp, seq: u64, event: WorldEvent) {
        if self.is_ghost(&event) {
            self.dismiss_ghost(&event);
            return;
        }
        let event = match event {
            WorldEvent::Crash { node, torn_extra } => {
                let torn = self.resolve_torn_extra(node, torn_extra);
                self.observed.crash_torn.insert((time.0, node.0), torn);
                WorldEvent::Crash {
                    node,
                    torn_extra: Some(torn),
                }
            }
            other => other,
        };
        self.trace.record(time, seq, &event);
        self.event_seq = seq;
        match event {
            WorldEvent::TimerFired { node, timer, .. } => {
                self.timers_stepped.push((node, timer));
                self.step_node(node, Event::TimerFired { timer });
            }
            WorldEvent::MessageDeliver {
                from,
                to,
                msg_id,
                msg,
            } => {
                self.net.take(msg_id);
                self.delivered.push((from, to, msg_id));
                self.messages.push((from, to, msg.clone()));
                self.step_node(to, Event::MessageReceived { from, msg });
            }
            WorldEvent::IoComplete {
                node,
                id,
                result,
                sync_len,
                ..
            } => {
                let Some(i) = self.idx(node) else {
                    return;
                };
                self.disks[i].complete(sync_len, result);
                self.applied_io.push((node, id, result));
                self.step_node(node, Event::IoComplete { id, result });
            }
            WorldEvent::Crash { node, torn_extra } => {
                let Some(i) = self.idx(node) else {
                    return;
                };
                let extra = torn_extra.unwrap_or(0) as usize;
                self.alive[i] = false;
                self.life[i] = self.life[i].saturating_add(1);
                self.bump_all_timers(node);
                self.disks[i].crash_torn_len(extra);
                self.reset_node(i, node);
                self.last_fault_at = time;
            }
            WorldEvent::Partition {
                from,
                to,
                connected,
                asymmetric,
            } => {
                self.net.set_connected(from, to, connected, asymmetric);
                self.last_fault_at = time;
            }
            WorldEvent::ClientInject { node, req } => {
                self.history.invoke(time, seq, &req);
                self.step_node(node, Event::ClientRequest { req });
            }
            WorldEvent::Recover { node } => {
                let Some(i) = self.idx(node) else {
                    return;
                };
                let durable = self.disks[i].recover_scan();
                self.alive[i] = true;
                self.reset_node(i, node);
                self.step_node(node, Event::Recover { durable });
            }
            WorldEvent::Dropped { .. } => {}
            WorldEvent::FailNextFsync { node } => {
                if let Some(i) = self.idx(node) {
                    self.disks[i].fail_next_fsync = true;
                }
            }
        }
        self.run_checks();
    }

    fn step_node(&mut self, node: NodeId, event: Event) {
        let Some(i) = self.idx(node) else {
            return;
        };
        let effects = self.nodes[i].step(event);
        self.interpret(node, effects);
        self.note_role(node);
    }

    fn interpret(&mut self, from: NodeId, effects: Vec<Effect>) {
        let now = self.sched.now();
        for effect in effects {
            match effect {
                Effect::IoSubmit { id, op } => {
                    let Some(i) = self.idx(from) else {
                        continue;
                    };
                    let pending = self.disks[i].apply_op(id, &op);
                    let delay = self.io_delay(from, &op);
                    let at = Timestamp(now.0.saturating_add(delay));
                    self.sched.enqueue(
                        at,
                        WorldEvent::IoComplete {
                            node: from,
                            id: pending.id,
                            result: pending.result,
                            sync_len: pending.sync_len,
                            life: self.life[i],
                        },
                    );
                }
                Effect::Send { to, msg } => {
                    self.observe_outgoing(from, to, &msg);
                    self.dispatch_send(from, to, msg);
                }
                Effect::ArmTimer { id, kind } => {
                    self.arm_timer(from, id, kind);
                }
                Effect::CancelTimer { id } => {
                    self.bump_timer(from, id);
                }
                Effect::Reply { to, request, resp } => {
                    let r = self
                        .history
                        .complete(now, self.event_seq, to, request, resp.clone());
                    self.note_check(r);
                    self.replies.push((to, resp));
                }
            }
        }
    }
}

fn make_node(id: NodeId, n: u8) -> Node {
    let peers = (0..n).map(NodeId).filter(|&p| p != id).collect();
    Node::new(id, peers)
}

pub fn run(seed: u64, cfg: SimConfig) -> [u8; 32] {
    let n = cfg.n;
    let mut cluster = Cluster::new(seed, cfg);
    for i in 0..n {
        cluster.inject_recover(NodeId(i));
    }
    cluster.drain();
    cluster.inject_client(
        NodeId(0),
        ClientReq {
            client: ClientId(1),
            request: RequestId(1),
            cmd: Cmd::Put {
                key: b"k".to_vec(),
                value: b"v".to_vec(),
            },
        },
    );
    cluster.drain();
    cluster.inject_client(
        NodeId(0),
        ClientReq {
            client: ClientId(1),
            request: RequestId(2),
            cmd: Cmd::Get { key: b"k".to_vec() },
        },
    );
    cluster.drain();
    if n >= 2 {
        cluster.inject_send(NodeId(0), NodeId(1), Message::Ping);
        cluster.drain();
    }
    cluster.inject_timer(NodeId(0), TimerId(1));
    cluster.drain();
    cluster.digest()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronos_protocol::{ClientReq, ClientResp, Cmd, RequestId, Role, TIMER_ELECTION};

    fn solo_node() -> Node {
        Node::new(NodeId(0), Vec::new())
    }

    fn become_leader(node: &mut Node, disk: &mut SimDisk) {
        drive(node, disk, Event::Recover { durable: vec![] });
        drive(
            node,
            disk,
            Event::TimerFired {
                timer: TIMER_ELECTION,
            },
        );
        assert_eq!(node.role(), Role::Leader);
    }

    fn put() -> Event {
        Event::ClientRequest {
            req: ClientReq {
                client: ClientId(1),
                request: RequestId(1),
                cmd: Cmd::Put {
                    key: b"k".to_vec(),
                    value: b"v".to_vec(),
                },
            },
        }
    }

    fn get_req(request: u64) -> Event {
        Event::ClientRequest {
            req: ClientReq {
                client: ClientId(1),
                request: RequestId(request),
                cmd: Cmd::Get { key: b"k".to_vec() },
            },
        }
    }

    fn get() -> Event {
        get_req(2)
    }

    fn quiet_cfg() -> SimConfig {
        SimConfig {
            n: 2,
            jitter_max_ns: 0,
            ..SimConfig::default()
        }
    }

    fn put_req() -> ClientReq {
        ClientReq {
            client: ClientId(1),
            request: RequestId(1),
            cmd: Cmd::Put {
                key: b"k".to_vec(),
                value: b"v".to_vec(),
            },
        }
    }

    fn get_client(request: u64) -> ClientReq {
        ClientReq {
            client: ClientId(1),
            request: RequestId(request),
            cmd: Cmd::Get { key: b"k".to_vec() },
        }
    }

    fn raft_cfg() -> SimConfig {
        SimConfig {
            n: 3,
            jitter_max_ns: 0,
            ..SimConfig::default()
        }
    }

    fn leader_of(cluster: &Cluster) -> Option<NodeId> {
        (0..cluster.nodes.len())
            .map(|i| NodeId(i as u8))
            .find(|&id| {
                cluster.alive(id) && cluster.node(id).is_some_and(|n| n.role() == Role::Leader)
            })
    }

    fn recover_all(cluster: &mut Cluster) {
        let n = cluster.nodes.len() as u8;
        for i in 0..n {
            cluster.inject_recover(NodeId(i));
        }
        cluster.drain();
    }

    #[test]
    fn put_get_via_sim_disk() {
        let mut node = solo_node();
        let mut disk = SimDisk::new();
        become_leader(&mut node, &mut disk);
        let put_replies = drive(&mut node, &mut disk, put());
        assert_eq!(
            put_replies,
            vec![(
                ClientId(1),
                ClientResp::Ok {
                    value: b"v".to_vec()
                }
            )]
        );
        let get_replies = drive(&mut node, &mut disk, get());
        assert_eq!(
            get_replies,
            vec![(
                ClientId(1),
                ClientResp::Ok {
                    value: b"v".to_vec()
                }
            )]
        );
    }

    #[test]
    fn fsync_err_then_covering_get_matches_recover() {
        let mut node = solo_node();
        let mut disk = SimDisk::new();
        become_leader(&mut node, &mut disk);
        disk.fail_next_fsync = true;
        let put_replies = drive(&mut node, &mut disk, put());
        assert_eq!(
            put_replies,
            vec![(
                ClientId(1),
                ClientResp::Err(chronos_protocol::ClientError::Io)
            )]
        );
        let get_replies = drive(&mut node, &mut disk, get());
        assert_eq!(
            get_replies,
            vec![(
                ClientId(1),
                ClientResp::Ok {
                    value: b"v".to_vec()
                }
            )]
        );
        let mut restarted = solo_node();
        let durable = disk.durable_prefix().to_vec();
        drive(&mut restarted, &mut disk, Event::Recover { durable });
        drive(
            &mut restarted,
            &mut disk,
            Event::TimerFired {
                timer: TIMER_ELECTION,
            },
        );
        let after = drive(&mut restarted, &mut disk, get_req(3));
        assert_eq!(
            after,
            vec![(
                ClientId(1),
                ClientResp::Ok {
                    value: b"v".to_vec()
                }
            )]
        );
    }

    #[test]
    fn crash_before_fsync_drops_put() {
        let mut node = solo_node();
        let mut disk = SimDisk::new();
        become_leader(&mut node, &mut disk);
        let durable_before = disk.durable_len;
        let effects = node.step(put());
        disk.submit(&effects);
        assert_eq!(disk.durable_len, durable_before);
        disk.crash();

        let mut restarted = solo_node();
        let durable = disk.durable_prefix().to_vec();
        drive(&mut restarted, &mut disk, Event::Recover { durable });
        drive(
            &mut restarted,
            &mut disk,
            Event::TimerFired {
                timer: TIMER_ELECTION,
            },
        );
        let get_replies = drive(&mut restarted, &mut disk, get());
        assert_eq!(
            get_replies,
            vec![(
                ClientId(1),
                ClientResp::Err(chronos_protocol::ClientError::NotFound)
            )]
        );
    }

    #[test]
    fn ping_from_0_to_1_delivered_once() {
        let mut cluster = Cluster::new(1, quiet_cfg());
        cluster.inject_recover(NodeId(0));
        cluster.inject_recover(NodeId(1));
        cluster.drain();
        cluster.inject_send(NodeId(0), NodeId(1), Message::Ping);
        cluster.drain();
        let ping = cluster.delivered().last().copied().expect("ping delivered");
        assert_eq!(ping.0, NodeId(0));
        assert_eq!(ping.1, NodeId(1));
    }

    #[test]
    fn heap_put_get_replies() {
        let mut cluster = Cluster::new(
            1,
            SimConfig {
                n: 1,
                jitter_max_ns: 0,
                ..SimConfig::default()
            },
        );
        cluster.inject_recover(NodeId(0));
        cluster.drain();
        cluster.inject_client(NodeId(0), put_req());
        cluster.drain();
        cluster.inject_client(NodeId(0), get_client(2));
        cluster.drain();
        assert_eq!(
            cluster.replies(),
            &[
                (
                    ClientId(1),
                    ClientResp::Ok {
                        value: b"v".to_vec()
                    }
                ),
                (
                    ClientId(1),
                    ClientResp::Ok {
                        value: b"v".to_vec()
                    }
                ),
            ]
        );
    }

    #[test]
    fn cancel_then_rearm_same_timer_does_not_deliver_stale_fire() {
        let mut cluster = Cluster::new(
            1,
            SimConfig {
                n: 1,
                jitter_max_ns: 0,
                ..SimConfig::default()
            },
        );
        cluster.inject_arm_timer(NodeId(0), TimerId(1), TimerKind::Heartbeat);
        cluster.inject_cancel_timer(NodeId(0), TimerId(1));
        cluster.inject_arm_timer(NodeId(0), TimerId(1), TimerKind::Heartbeat);
        cluster.drain();
        assert_eq!(cluster.timers_stepped(), &[(NodeId(0), TimerId(1))]);
    }

    #[test]
    fn heap_crash_before_fsync_drops_put_and_ignores_stale_complete() {
        let mut cluster = Cluster::new(
            1,
            SimConfig {
                n: 1,
                jitter_max_ns: 0,
                torn_suffix: false,
                ..SimConfig::default()
            },
        );
        cluster.inject_recover(NodeId(0));
        cluster.drain();
        let durable = cluster.durable_len(NodeId(0));
        cluster.inject_client(NodeId(0), put_req());
        cluster.inject_crash(NodeId(0));
        cluster.drain();
        assert!(!cluster.alive(NodeId(0)));
        assert_eq!(cluster.durable_len(NodeId(0)), durable);
        cluster.inject_recover(NodeId(0));
        cluster.drain();
        cluster.inject_client(NodeId(0), get_client(2));
        cluster.drain();
        assert_eq!(
            cluster.replies().last(),
            Some(&(
                ClientId(1),
                ClientResp::Err(chronos_protocol::ClientError::NotFound)
            ))
        );
    }

    #[test]
    fn partition_drops_send() {
        let mut cluster = Cluster::new(1, quiet_cfg());
        cluster.inject_recover(NodeId(0));
        cluster.inject_recover(NodeId(1));
        cluster.drain();
        cluster.inject_partition(NodeId(0), NodeId(1), false);
        cluster.drain();
        let n = cluster.delivered().len();
        cluster.inject_send(NodeId(0), NodeId(1), Message::Ping);
        cluster.drain();
        assert_eq!(cluster.delivered().len(), n);
    }

    #[test]
    fn run_same_seed_same_digest() {
        let a = run(42, SimConfig::default());
        let b = run(42, SimConfig::default());
        assert_eq!(a, b);
    }

    #[test]
    fn run_different_seeds_different_digest() {
        let a = run(42, SimConfig::default());
        let b = run(43, SimConfig::default());
        assert_ne!(a, b);
    }

    #[test]
    fn three_nodes_elect_one_leader() {
        let mut cluster = Cluster::new(7, raft_cfg());
        recover_all(&mut cluster);
        assert_eq!(
            (0..3)
                .filter(|&i| cluster
                    .node(NodeId(i))
                    .is_some_and(|n| n.role() == Role::Leader))
                .count(),
            1
        );
        assert_eq!(
            (0..3)
                .filter(|&i| cluster
                    .node(NodeId(i))
                    .is_some_and(|n| n.role() == Role::Follower))
                .count(),
            2
        );
    }

    #[test]
    fn put_on_leader_applied_on_all() {
        let mut cluster = Cluster::new(11, raft_cfg());
        recover_all(&mut cluster);
        let leader = leader_of(&cluster).expect("leader");
        cluster.inject_client(leader, put_req());
        cluster.drain();
        for i in 0..3 {
            assert_eq!(
                cluster.node(NodeId(i)).and_then(|n| n.kv_get(b"k")),
                Some(b"v".as_slice()),
                "node {i} missing put"
            );
        }
        cluster.inject_client(leader, get_client(2));
        cluster.drain();
        assert!(cluster.replies().iter().any(|(_, r)| {
            matches!(
                r,
                ClientResp::Ok { value } if value == b"v"
            )
        }));
    }

    #[test]
    fn duplicate_put_mutates_once() {
        let mut cluster = Cluster::new(13, raft_cfg());
        recover_all(&mut cluster);
        let leader = leader_of(&cluster).expect("leader");
        cluster.inject_client(leader, put_req());
        cluster.inject_client(leader, put_req());
        cluster.drain();
        let oks = cluster
            .replies()
            .iter()
            .filter(|(_, r)| matches!(r, ClientResp::Ok { value } if value == b"v"))
            .count();
        assert_eq!(oks, 2);
        assert_eq!(
            cluster.node(leader).and_then(|n| n.kv_get(b"k")),
            Some(b"v".as_slice())
        );
    }

    #[test]
    fn crash_down_does_not_recover_until_inject_recover() {
        let mut cluster = Cluster::new(
            2,
            SimConfig {
                n: 1,
                jitter_max_ns: 0,
                torn_suffix: false,
                ..SimConfig::default()
            },
        );
        cluster.inject_recover(NodeId(0));
        cluster.drain();
        cluster.inject_crash(NodeId(0));
        cluster.drain();
        assert!(!cluster.alive(NodeId(0)));
        cluster.inject_client(NodeId(0), put_req());
        cluster.drain();
        assert!(cluster.replies().is_empty());
        cluster.inject_recover(NodeId(0));
        cluster.drain();
        assert!(cluster.alive(NodeId(0)));
    }

    #[test]
    fn crash_and_reelect_reapplies_durable_put() {
        let mut cluster = Cluster::new(
            17,
            SimConfig {
                n: 1,
                jitter_max_ns: 0,
                ..SimConfig::default()
            },
        );
        recover_all(&mut cluster);
        cluster.inject_client(NodeId(0), put_req());
        cluster.drain();
        assert_eq!(
            cluster.node(NodeId(0)).and_then(|n| n.kv_get(b"k")),
            Some(b"v".as_slice())
        );
        let log_index = cluster.node(NodeId(0)).unwrap().last_log_index();
        cluster.inject_crash(NodeId(0));
        cluster.inject_recover(NodeId(0));
        cluster.drain();
        let node = cluster.node(NodeId(0)).unwrap();
        assert!(node.last_log_index() >= log_index);
        assert_eq!(node.kv_get(b"k"), Some(b"v".as_slice()));
    }

    #[test]
    fn unused_dup_and_torn_counters_start_unfired() {
        let mut cluster = Cluster::new(
            1,
            SimConfig {
                n: 1,
                jitter_max_ns: 0,
                dup_ppm: 50_000,
                torn_suffix: true,
                ..SimConfig::default()
            },
        );
        cluster.inject_recover(NodeId(0));
        cluster.drain();
        assert_eq!(cluster.dup_sends(), 0);
        assert!(!cluster.torn_applied());
    }

    #[test]
    fn specified_torn_extra_marks_torn_applied() {
        let mut cluster = Cluster::new(
            1,
            SimConfig {
                n: 1,
                jitter_max_ns: 0,
                torn_suffix: true,
                ..SimConfig::default()
            },
        );
        cluster.inject_recover(NodeId(0));
        cluster.drain();
        cluster.inject_at(
            cluster.now(),
            WorldEvent::Crash {
                node: NodeId(0),
                torn_extra: Some(1),
            },
        );
        cluster.drain();
        assert!(cluster.torn_applied());
    }

    #[test]
    fn schedule_replay_ignores_drop_ppm() {
        let mut cluster = Cluster::new(
            1,
            SimConfig {
                n: 2,
                drop_ppm: 1_000_000,
                jitter_max_ns: 0,
                ..SimConfig::default()
            },
        );
        cluster.use_schedule_replay(
            ReplayBook::default(),
            Vec::new(),
            Vec::new(),
            DelayBind::Defaults,
        );
        cluster.inject_recover(NodeId(0));
        cluster.inject_recover(NodeId(1));
        cluster.drain();
        let messages_before = cluster.messages().len();
        let dropped_before = cluster.dropped().len();
        cluster.inject_send(NodeId(0), NodeId(1), Message::Ping);
        cluster.drain();
        assert!(
            cluster
                .messages()
                .iter()
                .skip(messages_before)
                .any(|(from, to, msg)| {
                    *from == NodeId(0) && *to == NodeId(1) && *msg == Message::Ping
                }),
            "schedule replay must deliver Ping, not roll drop_ppm: dropped={:?}",
            cluster.dropped()
        );
        assert!(
            cluster
                .dropped()
                .iter()
                .skip(dropped_before)
                .all(|d| d.3 != DropReason::Loss),
            "empty drop tokens must not invent Loss"
        );
    }

    #[test]
    fn recorded_drop_token_is_causal() {
        let cfg = SimConfig {
            n: 2,
            drop_ppm: 1_000_000,
            jitter_max_ns: 0,
            ..SimConfig::default()
        };
        let mut swarm = Cluster::new(1, cfg.clone());
        swarm.inject_recover(NodeId(0));
        swarm.inject_recover(NodeId(1));
        swarm.drain();
        swarm.inject_send(NodeId(0), NodeId(1), Message::Ping);
        swarm.drain();
        let ping = delivery_token(NodeId(0), NodeId(1), &Message::Ping);
        let token = swarm
            .observed()
            .drops
            .iter()
            .copied()
            .find(|t| *t == ping)
            .expect("ppm=1e6 must record a Ping loss token");

        let mut with_drop = Cluster::new(1, cfg.clone());
        with_drop.use_schedule_replay(
            ReplayBook::default(),
            vec![token],
            Vec::new(),
            DelayBind::Defaults,
        );
        with_drop.inject_recover(NodeId(0));
        with_drop.inject_recover(NodeId(1));
        with_drop.drain();
        let messages_before = with_drop.messages().len();
        with_drop.inject_send(NodeId(0), NodeId(1), Message::Ping);
        with_drop.drain();
        assert!(
            with_drop
                .dropped()
                .iter()
                .any(|d| d.0 == NodeId(0) && d.1 == NodeId(1) && d.3 == DropReason::Loss),
            "token must drop Ping"
        );
        assert!(
            with_drop
                .messages()
                .iter()
                .skip(messages_before)
                .all(|(_, _, msg)| *msg != Message::Ping),
            "dropped Ping must not be delivered"
        );

        let mut without_drop = Cluster::new(1, cfg);
        without_drop.use_schedule_replay(
            ReplayBook::default(),
            Vec::new(),
            Vec::new(),
            DelayBind::Defaults,
        );
        without_drop.inject_recover(NodeId(0));
        without_drop.inject_recover(NodeId(1));
        without_drop.drain();
        let messages_before = without_drop.messages().len();
        without_drop.inject_send(NodeId(0), NodeId(1), Message::Ping);
        without_drop.drain();
        assert!(
            without_drop
                .messages()
                .iter()
                .skip(messages_before)
                .any(|(from, to, msg)| {
                    *from == NodeId(0) && *to == NodeId(1) && *msg == Message::Ping
                }),
            "removing the drop atom must deliver Ping"
        );
        assert!(without_drop
            .dropped()
            .iter()
            .all(|d| d.3 != DropReason::Loss));
    }
}
