//! Five Raft safety properties plus persist-before-send/ack. After every world event.
//!
//! Halt is the caller's job: these methods return `CheckFail` and never panic.
//! Spec: `docs/02-architecture.md` § Checkers.

use std::collections::BTreeMap;
use std::fmt;

use chronos_protocol::{
    Index, LogEntry, Message, Node, NodeId, PersistCookie, Role, Term, Timestamp,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckName {
    ElectionSafety,
    LeaderAppendOnly,
    LogMatching,
    LeaderCompleteness,
    StateMachineSafety,
    PersistBeforeSend,
    PersistBeforeAck,
    CommitIndexMonotonic,
    CurrentTermCommit,
    MatchIndexSelfFsync,
    Linearizability,
    CheckerCapacity,
    UnmatchedReply,
    Liveness,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeSnap {
    pub id: NodeId,
    pub alive: bool,
    pub life: u64,
    pub role: Role,
    pub term: Term,
    pub commit: Index,
    pub last_applied: Index,
    pub last_index: Index,
    pub durable: PersistCookie,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckFail {
    pub check: CheckName,
    pub detail: String,
    pub snapshots: Vec<NodeSnap>,
}

impl CheckFail {
    pub fn new(check: CheckName, detail: impl Into<String>) -> Self {
        Self {
            check,
            detail: detail.into(),
            snapshots: Vec::new(),
        }
    }
}

impl CheckName {
    /// Recorder/capacity faults. Not a Raft or linearizability finding.
    pub fn is_abort(self) -> bool {
        matches!(self, Self::CheckerCapacity | Self::UnmatchedReply)
    }

    pub fn as_label(self) -> &'static str {
        match self {
            Self::ElectionSafety => "ElectionSafety",
            Self::LeaderAppendOnly => "LeaderAppendOnly",
            Self::LogMatching => "LogMatching",
            Self::LeaderCompleteness => "LeaderCompleteness",
            Self::StateMachineSafety => "StateMachineSafety",
            Self::PersistBeforeSend => "PersistBeforeSend",
            Self::PersistBeforeAck => "PersistBeforeAck",
            Self::CommitIndexMonotonic => "CommitIndexMonotonic",
            Self::CurrentTermCommit => "CurrentTermCommit",
            Self::MatchIndexSelfFsync => "MatchIndexSelfFsync",
            Self::Linearizability => "Linearizability",
            Self::CheckerCapacity => "CheckerCapacity",
            Self::UnmatchedReply => "UnmatchedReply",
            Self::Liveness => "Liveness",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "ElectionSafety" => Some(Self::ElectionSafety),
            "LeaderAppendOnly" => Some(Self::LeaderAppendOnly),
            "LogMatching" => Some(Self::LogMatching),
            "LeaderCompleteness" => Some(Self::LeaderCompleteness),
            "StateMachineSafety" => Some(Self::StateMachineSafety),
            "PersistBeforeSend" => Some(Self::PersistBeforeSend),
            "PersistBeforeAck" => Some(Self::PersistBeforeAck),
            "CommitIndexMonotonic" => Some(Self::CommitIndexMonotonic),
            "CurrentTermCommit" => Some(Self::CurrentTermCommit),
            "MatchIndexSelfFsync" => Some(Self::MatchIndexSelfFsync),
            "Linearizability" => Some(Self::Linearizability),
            "CheckerCapacity" => Some(Self::CheckerCapacity),
            "UnmatchedReply" => Some(Self::UnmatchedReply),
            "Liveness" => Some(Self::Liveness),
            _ => None,
        }
    }
}

impl fmt::Display for CheckFail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.check, self.detail)
    }
}

pub struct CheckView<'a> {
    pub now: Timestamp,
    pub nodes: &'a [Node],
    pub alive: &'a [bool],
    pub life: &'a [u64],
}

#[derive(Clone, Debug)]
pub struct Checker {
    leaders_by_term: BTreeMap<Term, NodeId>,
    es_ok: bool,
    leader_log: BTreeMap<(Term, NodeId), Vec<LogEntry>>,
    committed: BTreeMap<Index, (Term, LogEntry)>,
    applied: BTreeMap<Index, LogEntry>,
    commit_hi: BTreeMap<(NodeId, u64), Index>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            leaders_by_term: BTreeMap::new(),
            es_ok: true,
            leader_log: BTreeMap::new(),
            committed: BTreeMap::new(),
            applied: BTreeMap::new(),
            commit_hi: BTreeMap::new(),
        }
    }

    pub fn election_safety_ok(&self) -> bool {
        self.es_ok
    }

    pub fn observe_send(
        &self,
        from: NodeId,
        to: NodeId,
        msg: &Message,
        durable: PersistCookie,
    ) -> Result<(), CheckFail> {
        match msg {
            Message::RequestVote { term, .. } => {
                if durable.term == *term && durable.voted_for == Some(from) {
                    Ok(())
                } else {
                    Err(CheckFail::new(
                        CheckName::PersistBeforeSend,
                        format!(
                            "node {from:?} sent RequestVote term {} with durable term={} voted_for={:?}",
                            term.0, durable.term.0, durable.voted_for
                        ),
                    ))
                }
            }
            Message::RequestVoteResp {
                term,
                granted: true,
            } => {
                if durable.term == *term && durable.voted_for == Some(to) {
                    Ok(())
                } else {
                    Err(CheckFail::new(
                        CheckName::PersistBeforeSend,
                        format!(
                            "node {from:?} granted vote to {to:?} term {} with durable term={} voted_for={:?}",
                            term.0, durable.term.0, durable.voted_for
                        ),
                    ))
                }
            }
            Message::AppendEntriesResp {
                success: true,
                match_index,
                ..
            } => {
                if durable.last_index >= *match_index {
                    Ok(())
                } else {
                    Err(CheckFail::new(
                        CheckName::PersistBeforeAck,
                        format!(
                            "node {from:?} acked match_index {} with durable last_index {}",
                            match_index.0, durable.last_index.0
                        ),
                    ))
                }
            }
            _ => Ok(()),
        }
    }

    pub fn after_event(&mut self, view: CheckView<'_>, engineering: bool) -> Result<(), CheckFail> {
        self.election_safety(&view)?;
        self.leader_append_only(&view)?;
        self.log_matching(&view)?;
        self.note_commits(&view, engineering)?;
        self.leader_completeness(&view)?;
        self.state_machine_safety(&view)?;
        if engineering {
            self.match_index_self_fsync(&view)?;
        }
        Ok(())
    }

    pub fn check_liveness(
        &self,
        now: Timestamp,
        last_fault: Timestamp,
        election_max_ns: u64,
        view: CheckView<'_>,
        majority_connected: bool,
    ) -> Result<(), CheckFail> {
        let window = election_max_ns.saturating_mul(10);
        if now.0.saturating_sub(last_fault.0) < window {
            return Ok(());
        }
        if !majority_connected {
            return Ok(());
        }
        let has_leader =
            view.nodes.iter().enumerate().any(|(i, n)| {
                view.alive.get(i).copied().unwrap_or(false) && n.role() == Role::Leader
            });
        if !has_leader {
            return Err(CheckFail::new(
                CheckName::Liveness,
                "majority connected long enough but no leader",
            ));
        }
        let wrote = view.nodes.iter().enumerate().any(|(i, n)| {
            view.alive.get(i).copied().unwrap_or(false) && n.commit_index() > Index(0)
        });
        if !wrote {
            return Err(CheckFail::new(
                CheckName::Liveness,
                "majority connected long enough but nothing committed",
            ));
        }
        Ok(())
    }

    fn election_safety(&mut self, view: &CheckView<'_>) -> Result<(), CheckFail> {
        for (i, node) in view.nodes.iter().enumerate() {
            if !alive_at(view, i) || node.role() != Role::Leader {
                continue;
            }
            let id = node.id();
            let term = node.current_term();
            match self.leaders_by_term.get(&term).copied() {
                None => {
                    self.leaders_by_term.insert(term, id);
                }
                Some(existing) if existing != id => {
                    self.es_ok = false;
                    return Err(CheckFail::new(
                        CheckName::ElectionSafety,
                        format!("two leaders in term {}: {existing:?} and {id:?}", term.0),
                    ));
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    fn leader_append_only(&mut self, view: &CheckView<'_>) -> Result<(), CheckFail> {
        for (i, node) in view.nodes.iter().enumerate() {
            if !alive_at(view, i) || node.role() != Role::Leader {
                continue;
            }
            let snap = node.log().entries().to_vec();
            let key = (node.current_term(), node.id());
            match self.leader_log.get(&key) {
                None => {
                    self.leader_log.insert(key, snap);
                }
                Some(old) => {
                    if snap.len() < old.len() {
                        return Err(CheckFail::new(
                            CheckName::LeaderAppendOnly,
                            format!(
                                "leader {:?} term {} log shrank {} -> {}",
                                node.id(),
                                node.current_term().0,
                                old.len(),
                                snap.len()
                            ),
                        ));
                    }
                    if snap[..old.len()] != old[..] {
                        return Err(CheckFail::new(
                            CheckName::LeaderAppendOnly,
                            format!(
                                "leader {:?} term {} overwrote a prefix",
                                node.id(),
                                node.current_term().0
                            ),
                        ));
                    }
                    self.leader_log.insert(key, snap);
                }
            }
        }
        Ok(())
    }

    fn log_matching(&self, view: &CheckView<'_>) -> Result<(), CheckFail> {
        let alive: Vec<&Node> = view
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| alive_at(view, *i))
            .map(|(_, n)| n)
            .collect();
        for (a, left) in alive.iter().enumerate() {
            for right in alive.iter().skip(a + 1) {
                let max = left.last_log_index().min(right.last_log_index());
                for i in 0..=max.0 {
                    let idx = Index(i);
                    let Some(lt) = left.log().term_at(idx) else {
                        continue;
                    };
                    let Some(rt) = right.log().term_at(idx) else {
                        continue;
                    };
                    if lt != rt {
                        continue;
                    }
                    for k in 0..=i {
                        let ki = Index(k);
                        if left.log().entry(ki) != right.log().entry(ki) {
                            return Err(CheckFail::new(
                                CheckName::LogMatching,
                                format!(
                                    "nodes {:?} and {:?} share ({}, term {}) but prefixes differ at {}",
                                    left.id(),
                                    right.id(),
                                    i,
                                    lt.0,
                                    k
                                ),
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn note_commits(&mut self, view: &CheckView<'_>, engineering: bool) -> Result<(), CheckFail> {
        for (i, node) in view.nodes.iter().enumerate() {
            if !alive_at(view, i) {
                continue;
            }
            let life = view.life.get(i).copied().unwrap_or(0);
            let id = node.id();
            let key = (id, life);
            let prev = self.commit_hi.get(&key).copied().unwrap_or(Index(0));
            let cur = node.commit_index();
            if cur < prev {
                if engineering {
                    return Err(CheckFail::new(
                        CheckName::CommitIndexMonotonic,
                        format!(
                            "node {id:?} life {life} commitIndex {} -> {}",
                            prev.0, cur.0
                        ),
                    ));
                }
            } else if cur > prev {
                if node.role() == Role::Leader {
                    if engineering {
                        match node.log().entry(cur) {
                            Some(e) if e.term == node.current_term() => {}
                            Some(e) => {
                                return Err(CheckFail::new(
                                    CheckName::CurrentTermCommit,
                                    format!(
                                        "leader {id:?} committed index {} term {} while currentTerm {}",
                                        cur.0,
                                        e.term.0,
                                        node.current_term().0
                                    ),
                                ));
                            }
                            None => {
                                return Err(CheckFail::new(
                                    CheckName::CurrentTermCommit,
                                    format!("leader {id:?} commitIndex {} past log", cur.0),
                                ));
                            }
                        }
                    }
                    for n in prev.0.saturating_add(1)..=cur.0 {
                        let idx = Index(n);
                        if let Some(entry) = node.log().entry(idx) {
                            self.committed
                                .entry(idx)
                                .or_insert((node.current_term(), entry.clone()));
                        }
                    }
                }
                self.commit_hi.insert(key, cur);
            }
        }
        Ok(())
    }

    fn leader_completeness(&self, view: &CheckView<'_>) -> Result<(), CheckFail> {
        for (idx, (term, entry)) in &self.committed {
            for (i, node) in view.nodes.iter().enumerate() {
                if !alive_at(view, i) || node.role() != Role::Leader {
                    continue;
                }
                if node.current_term() <= *term {
                    continue;
                }
                match node.log().entry(*idx) {
                    Some(e) if e == entry => {}
                    other => {
                        return Err(CheckFail::new(
                            CheckName::LeaderCompleteness,
                            format!(
                                "term {} committed {:?} at {} but later leader {:?} has {other:?}",
                                term.0,
                                entry.term,
                                idx.0,
                                node.id()
                            ),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn state_machine_safety(&mut self, view: &CheckView<'_>) -> Result<(), CheckFail> {
        for (i, node) in view.nodes.iter().enumerate() {
            if !alive_at(view, i) {
                continue;
            }
            let last = node.last_applied();
            for n in 1..=last.0 {
                let idx = Index(n);
                let Some(entry) = node.log().entry(idx) else {
                    return Err(CheckFail::new(
                        CheckName::StateMachineSafety,
                        format!(
                            "node {:?} last_applied {} missing log entry {}",
                            node.id(),
                            last.0,
                            n
                        ),
                    ));
                };
                match self.applied.get(&idx) {
                    None => {
                        self.applied.insert(idx, entry.clone());
                    }
                    Some(prev) if prev != entry => {
                        return Err(CheckFail::new(
                            CheckName::StateMachineSafety,
                            format!("index {} applied {:?} then {:?}", n, prev.term, entry.term),
                        ));
                    }
                    Some(_) => {}
                }
            }
        }
        let alive: Vec<&Node> = view
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| alive_at(view, *i))
            .map(|(_, n)| n)
            .collect();
        for (a, left) in alive.iter().enumerate() {
            for right in alive.iter().skip(a + 1) {
                if left.last_applied() != right.last_applied() || left.last_applied() == Index(0) {
                    continue;
                }
                if left.kv_store() != right.kv_store() || left.idempotency() != right.idempotency()
                {
                    return Err(CheckFail::new(
                        CheckName::StateMachineSafety,
                        format!(
                            "nodes {:?} and {:?} share last_applied {} but KV/idempotency differ",
                            left.id(),
                            right.id(),
                            left.last_applied().0
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    fn match_index_self_fsync(&self, view: &CheckView<'_>) -> Result<(), CheckFail> {
        for (i, node) in view.nodes.iter().enumerate() {
            if !alive_at(view, i) {
                continue;
            }
            if node.match_index_self() > node.durable().last_index {
                return Err(CheckFail::new(
                    CheckName::MatchIndexSelfFsync,
                    format!(
                        "node {:?} matchIndex[self] {} > durable last_index {}",
                        node.id(),
                        node.match_index_self().0,
                        node.durable().last_index.0
                    ),
                ));
            }
        }
        Ok(())
    }
}

impl Default for Checker {
    fn default() -> Self {
        Self::new()
    }
}

fn alive_at(view: &CheckView<'_>, i: usize) -> bool {
    view.alive.get(i).copied().unwrap_or(false)
}

pub fn majority_fully_connected(
    n: u8,
    alive: &[bool],
    connected: impl Fn(NodeId, NodeId) -> bool,
) -> bool {
    let need = usize::from(n) / 2 + 1;
    let ids: Vec<NodeId> = (0..n)
        .map(NodeId)
        .filter(|id| alive.get(usize::from(id.0)).copied().unwrap_or(false))
        .collect();
    if ids.len() < need {
        return false;
    }
    let m = ids.len();
    if m >= 31 {
        return false;
    }
    let limit = 1u32 << m;
    for mask in 1..limit {
        if mask.count_ones() as usize != need {
            continue;
        }
        let mut ok = true;
        for a in 0..m {
            if mask & (1 << a) == 0 {
                continue;
            }
            for b in 0..m {
                if a == b || mask & (1 << b) == 0 {
                    continue;
                }
                if !connected(ids[a], ids[b]) {
                    ok = false;
                    break;
                }
            }
            if !ok {
                break;
            }
        }
        if ok {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronos_protocol::{NodeId, PersistCookie, Term, Timestamp};

    #[test]
    fn request_vote_without_durable_cookie_fails() {
        let c = Checker::new();
        let msg = Message::RequestVote {
            term: Term(1),
            last_log_index: Index(0),
            last_log_term: Term(0),
        };
        let err = c
            .observe_send(NodeId(0), NodeId(1), &msg, PersistCookie::default())
            .unwrap_err();
        assert_eq!(err.check, CheckName::PersistBeforeSend);
    }

    #[test]
    fn capacity_and_unmatched_reply_are_harness_aborts() {
        assert!(CheckName::CheckerCapacity.is_abort());
        assert!(CheckName::UnmatchedReply.is_abort());
        assert!(!CheckName::ElectionSafety.is_abort());
        assert!(!CheckName::Linearizability.is_abort());
    }

    #[test]
    fn check_name_label_roundtrips() {
        assert_eq!(
            CheckName::from_label(CheckName::ElectionSafety.as_label()),
            Some(CheckName::ElectionSafety)
        );
        assert_eq!(
            CheckName::from_label(CheckName::UnmatchedReply.as_label()),
            Some(CheckName::UnmatchedReply)
        );
        assert_eq!(CheckName::from_label("none"), None);
        assert_eq!(CheckName::from_label("not-a-check"), None);
    }

    #[test]
    fn request_vote_with_covering_cookie_passes() {
        let c = Checker::new();
        let msg = Message::RequestVote {
            term: Term(1),
            last_log_index: Index(0),
            last_log_term: Term(0),
        };
        let durable = PersistCookie {
            term: Term(1),
            voted_for: Some(NodeId(0)),
            last_index: Index(0),
            last_term: Term(0),
        };
        assert!(c.observe_send(NodeId(0), NodeId(1), &msg, durable).is_ok());
    }

    #[test]
    fn ae_success_before_durable_index_fails() {
        let c = Checker::new();
        let msg = Message::AppendEntriesResp {
            term: Term(1),
            success: true,
            match_index: Index(3),
        };
        let err = c
            .observe_send(NodeId(1), NodeId(0), &msg, PersistCookie::default())
            .unwrap_err();
        assert_eq!(err.check, CheckName::PersistBeforeAck);
    }

    #[test]
    fn majority_clique_of_two_in_three() {
        let alive = [true, true, false];
        assert!(majority_fully_connected(3, &alive, |_, _| true));
        assert!(!majority_fully_connected(3, &alive, |_, _| false));
    }

    #[test]
    fn liveness_is_vacuous_before_the_window() {
        let c = Checker::new();
        let nodes: [Node; 0] = [];
        let alive: [bool; 0] = [];
        let life: [u64; 0] = [];
        let view = CheckView {
            now: Timestamp(100),
            nodes: &nodes,
            alive: &alive,
            life: &life,
        };
        assert!(c
            .check_liveness(Timestamp(100), Timestamp(0), 50, view, true)
            .is_ok());
    }

    #[test]
    fn liveness_is_vacuous_without_a_connected_majority() {
        let c = Checker::new();
        let nodes: [Node; 0] = [];
        let alive: [bool; 0] = [];
        let life: [u64; 0] = [];
        let view = CheckView {
            now: Timestamp(1_000),
            nodes: &nodes,
            alive: &alive,
            life: &life,
        };
        assert!(c
            .check_liveness(Timestamp(1_000), Timestamp(0), 50, view, false)
            .is_ok());
    }
}
