//! P5 property checkers: planted votedFor skip, linearizability, liveness.
//!
//! Spec: `docs/roadmap/P05-properties.md`. Cluster heap only.

use chronos_protocol::{
    ClientId, ClientReq, Cmd, Message, NodeId, RequestId, Role, Timestamp, TIMER_ELECTION,
};
use chronos_sim::{CheckName, Cluster, SimConfig};

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

fn put(client: u64, request: u64, key: &[u8], value: &[u8]) -> ClientReq {
    ClientReq {
        client: ClientId(client),
        request: RequestId(request),
        cmd: Cmd::Put {
            key: key.to_vec(),
            value: value.to_vec(),
        },
    }
}

fn get(client: u64, request: u64, key: &[u8]) -> ClientReq {
    ClientReq {
        client: ClientId(client),
        request: RequestId(request),
        cmd: Cmd::Get { key: key.to_vec() },
    }
}

fn assert_ok(cluster: &Cluster) {
    assert!(
        cluster.check_fail().is_none(),
        "check failed: {:?}",
        cluster.check_fail()
    );
}

/// Predict: sending RequestVote before Meta fsync fails persist-before-send.
#[test]
fn planted_skip_vote_persist_fails_persist_before_send() {
    let mut cluster = Cluster::new(3, raft_cfg());
    cluster.set_skip_vote_persist(NodeId(0), true);
    cluster.inject_recover(NodeId(0));
    cluster.drain();
    let fail = cluster.check_fail().expect("persist-before-send");
    assert_eq!(fail.check, CheckName::PersistBeforeSend);
    assert!(
        !fail.snapshots.is_empty(),
        "fail should snapshot nodes: {fail:?}"
    );
}

/// Predict: the same schedule with the hook off does not fail persist-before-send.
#[test]
fn unplanted_solo_campaign_passes_persist_before_send() {
    let mut cluster = Cluster::new(3, raft_cfg());
    cluster.inject_recover(NodeId(0));
    cluster.drain();
    assert_ok(&cluster);
    assert!(cluster.check_fail().is_none());
}

/// Predict: crash before vote persist, recover, vote for another candidate in
/// the same term → two leaders of that term.
#[test]
fn planted_skip_vote_persist_fails_election_safety() {
    let mut cluster = Cluster::new(
        5,
        SimConfig {
            n: 3,
            jitter_max_ns: 0,
            check_engineering: false,
            io_delay_min_ns: 5_000_000,
            io_delay_max_ns: 5_000_000,
            net_delay_min_ns: 0,
            net_delay_max_ns: 0,
            election_min_ns: 150_000_000,
            election_max_ns: 150_000_000,
            max_ns: 2_000_000_000,
            ..SimConfig::default()
        },
    );
    cluster.set_skip_vote_persist(NodeId(0), true);
    cluster.inject_partition(NodeId(1), NodeId(2), false);
    cluster.inject_partition(NodeId(0), NodeId(2), false);
    cluster.inject_recover(NodeId(0));
    cluster.inject_recover(NodeId(1));
    cluster.inject_recover(NodeId(2));
    while cluster.peek_time() == Some(Timestamp(0)) {
        assert!(
            cluster.step_once(),
            "t=0 recover/partition; check={:?}",
            cluster.check_fail()
        );
    }
    assert!(
        cluster.peek_time().is_some_and(|t| t > Timestamp(0)),
        "recovers should have armed later election timers"
    );
    assert!(
        !cluster.connected(NodeId(1), NodeId(2)) && !cluster.connected(NodeId(0), NodeId(2)),
        "partitions should have been applied"
    );
    cluster.inject_cancel_timer(NodeId(0), TIMER_ELECTION);
    cluster.inject_cancel_timer(NodeId(2), TIMER_ELECTION);
    cluster.inject_timer(NodeId(1), TIMER_ELECTION);

    let mut saw_grant = false;
    for _ in 0..10_000 {
        if cluster.messages().iter().any(|(from, _, msg)| {
            *from == NodeId(0) && matches!(msg, Message::RequestVoteResp { granted: true, .. })
        }) {
            saw_grant = true;
            break;
        }
        assert!(
            cluster.step_once(),
            "heap idle before node 0 granted a vote"
        );
    }
    assert!(saw_grant, "node 0 should grant before crash");
    cluster.inject_crash(NodeId(0));
    for _ in 0..10_000 {
        if !cluster.alive(NodeId(0)) {
            break;
        }
        assert!(cluster.step_once(), "heap idle before crash applied");
    }
    assert!(!cluster.alive(NodeId(0)));

    cluster.inject_recover(NodeId(0));
    cluster.inject_partition(NodeId(0), NodeId(1), false);
    cluster.inject_partition(NodeId(0), NodeId(2), true);
    cluster.inject_timer(NodeId(2), TIMER_ELECTION);
    for _ in 0..20_000 {
        if cluster.check_fail().map(|f| f.check) == Some(CheckName::ElectionSafety) {
            break;
        }
        if !cluster.step_once() {
            break;
        }
    }
    assert_eq!(
        cluster.check_fail().map(|f| f.check),
        Some(CheckName::ElectionSafety),
        "{:?}",
        cluster.check_fail()
    );
}

/// Predict: unplanted 3-node happy path passes every safety + engineering check.
#[test]
fn unplanted_happy_path_passes_all_checks() {
    let mut cluster = Cluster::new(7, raft_cfg());
    recover_all(&mut cluster);
    assert_ok(&cluster);
    let leader = leader_of(&cluster).expect("leader");
    cluster.inject_client(leader, put(1, 1, b"k", b"v"));
    cluster.drain();
    assert_ok(&cluster);
    let leader = leader_of(&cluster).expect("leader");
    cluster.inject_client(leader, get(1, 2, b"k"));
    cluster.drain();
    assert_ok(&cluster);
}

/// Predict: concurrent Put/Get from two clients is linearizable (no faults).
#[test]
fn concurrent_put_get_history_is_linearizable() {
    let mut cluster = Cluster::new(11, raft_cfg());
    recover_all(&mut cluster);
    let leader = leader_of(&cluster).expect("leader");
    cluster.inject_client(leader, put(1, 1, b"k", b"a"));
    cluster.inject_client(leader, put(2, 1, b"k", b"b"));
    cluster.drain();
    assert_ok(&cluster);
    let leader = leader_of(&cluster).expect("leader");
    cluster.inject_client(leader, get(1, 2, b"k"));
    cluster.inject_client(leader, get(2, 2, b"k"));
    cluster.drain();
    assert_ok(&cluster);
}

/// Predict: a healthy majority for 10 election timeouts elects and commits.
#[test]
fn liveness_healthy_cluster_elects_and_commits() {
    let mut cluster = Cluster::new(
        13,
        SimConfig {
            n: 3,
            jitter_max_ns: 0,
            check_liveness: true,
            max_ns: 4_000_000_000,
            ..SimConfig::default()
        },
    );
    recover_all(&mut cluster);
    assert_ok(&cluster);
    let leader = leader_of(&cluster).expect("leader");
    cluster.inject_client(leader, put(1, 1, b"k", b"v"));
    cluster.drain();
    assert_ok(&cluster);
}

/// Predict: liveness is vacuously true until 10 * election_max of stability.
#[test]
fn liveness_before_stable_window_is_ok() {
    let mut cluster = Cluster::new(
        17,
        SimConfig {
            n: 3,
            jitter_max_ns: 0,
            check_liveness: true,
            max_ns: 1,
            ..SimConfig::default()
        },
    );
    recover_all(&mut cluster);
    assert_ok(&cluster);
}
