//! P4 scripted fault scenarios. Each test states a prediction.
//!
//! Spec: `docs/roadmap/P04-faults.md`. Cluster heap only; not `drive()`.

use chronos_protocol::{
    ClientId, ClientReq, ClientResp, Cmd, Message, NodeId, RequestId, Role, Timestamp,
};
use chronos_sim::{Cluster, DropRule, RpcKind, SimConfig, WorldEvent};

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

fn get_req(request: u64) -> ClientReq {
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

fn recover_all(cluster: &mut Cluster) {
    for i in 0..cluster.node_count() {
        cluster.inject_recover(NodeId(i));
    }
    cluster.drain();
}

fn leader_of(cluster: &Cluster) -> Option<NodeId> {
    (0..cluster.node_count())
        .map(NodeId)
        .find(|&id| cluster.alive(id) && cluster.node(id).is_some_and(|n| n.role() == Role::Leader))
}

fn assert_election_safety(cluster: &Cluster) {
    assert!(
        cluster.check_fail().is_none(),
        "check failed: {:?}",
        cluster.check_fail()
    );
    assert!(cluster.election_safety_ok(), "two leaders in the same term");
}

fn others(n: u8, leader: NodeId) -> impl Iterator<Item = NodeId> {
    (0..n).map(NodeId).filter(move |&id| id != leader)
}

/// Predict: majority elects a new leader; old leader steps down on heal;
/// at most one leader; uncommitted minority entries are not required to survive.
#[test]
fn partition_leader_from_majority() {
    let mut cluster = Cluster::new(21, raft_cfg());
    recover_all(&mut cluster);
    assert_election_safety(&cluster);
    let leader = leader_of(&cluster).expect("leader");
    let n = cluster.node_count();
    let pre_term = cluster.node(leader).unwrap().current_term();
    cluster.inject_client(leader, put_req());
    cluster.drain();
    for i in 0..n {
        assert_eq!(
            cluster.node(NodeId(i)).and_then(|nd| nd.kv_get(b"k")),
            Some(b"v".as_slice())
        );
    }

    let t = cluster.now();
    let hold = Timestamp(t.0.saturating_add(1_000_000_000));
    for peer in others(n, leader) {
        cluster.inject_at(
            t,
            WorldEvent::Partition {
                from: leader,
                to: peer,
                connected: false,
                asymmetric: false,
            },
        );
        cluster.inject_at(
            hold,
            WorldEvent::Partition {
                from: leader,
                to: peer,
                connected: true,
                asymmetric: false,
            },
        );
    }
    cluster.drain();
    assert_election_safety(&cluster);
    let new_leader = leader_of(&cluster).expect("leader after heal");
    let majority = usize::from(n) / 2 + 1;
    let term_bumped = (0..n)
        .map(NodeId)
        .filter(|&id| {
            cluster
                .node(id)
                .is_some_and(|nd| nd.current_term() > pre_term)
        })
        .count();
    assert!(
        new_leader != leader || term_bumped >= majority,
        "expected a new leader or a majority term bump after partition heal"
    );
    if new_leader != leader {
        assert_ne!(cluster.node(leader).map(|nd| nd.role()), Some(Role::Leader));
    }
    let reader = new_leader;
    cluster.inject_client(reader, get_req(2));
    cluster.drain();
    assert!(cluster
        .replies()
        .iter()
        .any(|(_, r)| { matches!(r, ClientResp::Ok { value } if value == b"v") }));
}

/// Predict: delayed RPCs trigger an election; the cluster then makes progress.
#[test]
fn message_delay_triggers_election_then_progress() {
    let mut cluster = Cluster::new(
        23,
        SimConfig {
            n: 3,
            jitter_max_ns: 0,
            net_delay_min_ns: 400_000_000,
            net_delay_max_ns: 400_000_000,
            election_min_ns: 800_000_000,
            election_max_ns: 900_000_000,
            ..SimConfig::default()
        },
    );
    recover_all(&mut cluster);
    assert_election_safety(&cluster);
    let leader = leader_of(&cluster).expect("leader");
    cluster.inject_client(leader, put_req());
    cluster.drain();
    assert_election_safety(&cluster);
    assert!(leader_of(&cluster).is_some());
    let any = leader_of(&cluster).unwrap();
    cluster.inject_client(any, get_req(2));
    cluster.drain();
    assert!(cluster
        .replies()
        .iter()
        .any(|(_, r)| { matches!(r, ClientResp::Ok { value } if value == b"v") }));
}

/// Predict: dropping node 0's votes lets the other two elect; safety holds.
#[test]
fn drop_all_votes_for_one_candidate() {
    let mut cluster = Cluster::new(29, raft_cfg());
    cluster.add_drop_rule(DropRule {
        from: Some(NodeId(0)),
        to: None,
        kind: Some(RpcKind::RequestVote),
    });
    recover_all(&mut cluster);
    assert_election_safety(&cluster);
    assert!(leader_of(&cluster).is_some());
    assert!(!cluster.messages().iter().any(|(from, _, msg)| {
        *from == NodeId(0) && matches!(msg, Message::RequestVote { .. })
    }));
    assert!(cluster
        .dropped()
        .iter()
        .any(|(from, _, _, _)| *from == NodeId(0)));
}

/// Predict: a ping already on the wire is delivered after the sender crashes;
/// the crashed node's disk completions are not.
#[test]
fn inflight_message_survives_sender_crash() {
    let mut cluster = Cluster::new(
        31,
        SimConfig {
            n: 2,
            jitter_max_ns: 0,
            net_delay_min_ns: 1_000_000,
            net_delay_max_ns: 1_000_000,
            ..SimConfig::default()
        },
    );
    cluster.inject_recover(NodeId(0));
    cluster.inject_recover(NodeId(1));
    cluster.drain();
    let before = cluster.delivered().len();
    let applied_0 = cluster
        .applied_io()
        .iter()
        .filter(|(n, _, _)| *n == NodeId(0))
        .count();
    cluster.inject_send(NodeId(0), NodeId(1), Message::Ping);
    cluster.inject_crash(NodeId(0));
    cluster.drain();
    assert!(!cluster.alive(NodeId(0)));
    assert!(cluster.alive(NodeId(1)));
    assert!(cluster
        .delivered()
        .iter()
        .skip(before)
        .any(|&(_, to, _)| to == NodeId(1)));
    assert_eq!(
        cluster
            .applied_io()
            .iter()
            .filter(|(n, _, _)| *n == NodeId(0))
            .count(),
        applied_0,
        "crashed node's disk completions must not apply"
    );
}

/// Predict: vote Meta Fsync Err means no RequestVote from that node.
#[test]
fn vote_fsync_err_does_not_send_request_vote() {
    let mut cluster = Cluster::new(37, raft_cfg());
    cluster.inject_recover(NodeId(0));
    cluster.inject_recover(NodeId(1));
    cluster.inject_recover(NodeId(2));
    cluster.fail_next_fsync(NodeId(0));
    loop {
        if cluster
            .applied_io()
            .iter()
            .any(|(n, _, r)| *n == NodeId(0) && r.is_err())
        {
            break;
        }
        assert!(
            cluster.step_once(),
            "expected node 0 fsync Err before the heap went idle"
        );
    }
    assert!(!cluster.messages().iter().any(|(from, _, msg)| {
        *from == NodeId(0) && matches!(msg, Message::RequestVote { .. })
    }));
    assert_election_safety(&cluster);
}

/// Predict: committed Put remains readable after leader crash and delayed restart.
#[test]
fn crash_leader_restart_after_delay_keeps_committed_put() {
    let mut cluster = Cluster::new(41, raft_cfg());
    recover_all(&mut cluster);
    let leader = leader_of(&cluster).expect("leader");
    cluster.inject_client(leader, put_req());
    cluster.drain();
    for i in 0..cluster.node_count() {
        assert_eq!(
            cluster.node(NodeId(i)).and_then(|n| n.kv_get(b"k")),
            Some(b"v".as_slice()),
            "node {i} missing committed put"
        );
    }
    let t = cluster.now();
    let restart = Timestamp(t.0.saturating_add(200_000_000));
    cluster.inject_at(
        t,
        WorldEvent::Crash {
            node: leader,
            torn_extra: None,
        },
    );
    cluster.inject_at(restart, WorldEvent::Recover { node: leader });
    cluster.drain();
    assert_election_safety(&cluster);
    let reader = leader_of(&cluster).expect("leader after crash");
    cluster.inject_client(reader, get_req(2));
    cluster.drain();
    assert!(cluster
        .replies()
        .iter()
        .any(|(_, r)| { matches!(r, ClientResp::Ok { value } if value == b"v") }));
}

/// Characterization: fsync-lie reports Ok but crash loses the tail. Not a v1 bug to fix.
#[test]
fn fsync_lie_ok_then_crash_loses_put() {
    let mut cluster = Cluster::new(
        43,
        SimConfig {
            n: 1,
            jitter_max_ns: 0,
            torn_suffix: false,
            fsync_ok_but_not_durable: true,
            check_safety: false,
            ..SimConfig::default()
        },
    );
    recover_all(&mut cluster);
    cluster.inject_client(NodeId(0), put_req());
    cluster.drain();
    assert_eq!(
        cluster.node(NodeId(0)).and_then(|n| n.kv_get(b"k")),
        Some(b"v".as_slice()),
        "protocol believed the lying fsync"
    );
    cluster.inject_crash(NodeId(0));
    cluster.inject_recover(NodeId(0));
    cluster.drain();
    cluster.inject_client(NodeId(0), get_req(2));
    cluster.drain();
    assert_eq!(
        cluster.replies().last(),
        Some(&(
            ClientId(1),
            ClientResp::Err(chronos_protocol::ClientError::NotFound)
        ))
    );
}

/// Predict: buggify slow fsync still elects and commits (safety, not liveness bound).
#[test]
fn buggify_slow_fsync_still_safe() {
    let mut cluster = Cluster::new(
        47,
        SimConfig {
            n: 3,
            jitter_max_ns: 0,
            buggify_slow_fsync: true,
            buggify_fsync_extra_ns: 150_000_000,
            election_min_ns: 400_000_000,
            election_max_ns: 500_000_000,
            ..SimConfig::default()
        },
    );
    recover_all(&mut cluster);
    assert_election_safety(&cluster);
    let leader = leader_of(&cluster).expect("leader");
    cluster.inject_client(leader, put_req());
    cluster.drain();
    assert_election_safety(&cluster);
    let committed = (0..cluster.node_count())
        .filter(|&i| {
            cluster
                .node(NodeId(i))
                .and_then(|n| n.kv_get(b"k"))
                .is_some()
        })
        .count();
    assert!(committed >= 2, "majority should have the committed put");
}
