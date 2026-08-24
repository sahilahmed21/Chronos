//! Delta-debug the fault/delivery schedule, not Raft internals.
//!
//! Schedule replay uses recorded tokens + a delay book. World `Rng` is unread.
//! Spec: `docs/02-architecture.md` § Fuzzing and minimization. P8.

use chronos_protocol::{ClientReq, NodeId, Timestamp};

use crate::check::CheckName;
use crate::cluster::{Cluster, DelayBind, DeliveryToken, ObservedSchedule, ReplayBook, SimConfig};
use crate::fuzz::{drain_plan, finish_report, swarm_plan, Profile, RunReport};
use crate::scheduler::WorldEvent;
use crate::trace::digest_hex;

const MAX_CANDIDATE_DRAINS: u32 = 10_000;

/// One extra world decision the minifier may delete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Atom {
    CrashWindow {
        orig: usize,
        orig_up: Option<usize>,
        node: NodeId,
        down_at: Timestamp,
        up_at: Option<Timestamp>,
        torn: u64,
    },
    PartitionWindow {
        orig: usize,
        orig_heal: Option<usize>,
        from: NodeId,
        to: NodeId,
        cut_at: Timestamp,
        heal_at: Option<Timestamp>,
        asymmetric: bool,
    },
    PartitionHeal {
        orig: usize,
        t: Timestamp,
        from: NodeId,
        to: NodeId,
        asymmetric: bool,
    },
    Client {
        orig: usize,
        t: Timestamp,
        node: NodeId,
        req: ClientReq,
    },
    Recover {
        orig: usize,
        t: Timestamp,
        node: NodeId,
    },
    FailNextFsync {
        orig: usize,
        t: Timestamp,
        node: NodeId,
    },
    Drop(DeliveryToken),
    Dup(DeliveryToken),
}

#[derive(Clone, Debug)]
pub struct MinifyInput {
    pub seed: u64,
    pub profile: Profile,
    pub cfg: SimConfig,
    pub extras: Vec<(Timestamp, WorldEvent)>,
}

struct MinifyJob {
    input: MinifyInput,
    skip_vote_persist: Vec<(NodeId, bool)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinResult {
    pub seed: u64,
    pub check: CheckName,
    pub atoms_before: usize,
    pub atoms_after: usize,
    pub extras_before: usize,
    pub extras_after: usize,
    pub rounds: u32,
    pub capped: bool,
    pub atoms: Vec<Atom>,
    pub extras: Vec<(Timestamp, WorldEvent)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MinifyOutcome {
    Clean,
    Abort { check: CheckName },
    HarnessMismatch,
    Minified(MinResult),
}

pub fn minify(seed: u64) -> MinifyOutcome {
    let plan = swarm_plan(seed);
    minify_input(MinifyInput {
        seed,
        profile: plan.profile,
        cfg: plan.cfg,
        extras: plan.extras,
    })
}

pub fn minify_input(input: MinifyInput) -> MinifyOutcome {
    minify_job(MinifyJob {
        input,
        skip_vote_persist: Vec::new(),
    })
}

#[cfg(test)]
fn minify_planted(input: MinifyInput, skip_vote_persist: Vec<(NodeId, bool)>) -> MinifyOutcome {
    minify_job(MinifyJob {
        input,
        skip_vote_persist,
    })
}

fn minify_job(job: MinifyJob) -> MinifyOutcome {
    let cluster = drain_plan(
        job.input.seed,
        &job.input.cfg,
        &job.input.extras,
        &job.skip_vote_persist,
    );
    let Some(fail) = cluster.check_fail() else {
        return MinifyOutcome::Clean;
    };
    if fail.check.is_abort() {
        return MinifyOutcome::Abort { check: fail.check };
    }
    let target = fail.check;
    let observed = cluster.observed().clone();
    let atoms = atomize(&job.input.extras, &observed);
    let book = observed.book.clone();
    let extras_before = job.input.extras.len();
    let atoms_before = atoms.len();
    if !schedule_fails(&job, &atoms, Some(&book), DelayBind::Recorded, target) {
        return MinifyOutcome::HarnessMismatch;
    }
    let mut rounds = 0u32;
    let (min_atoms, capped) = delta_debug(&job, atoms.clone(), target, &mut rounds);
    if min_atoms != atoms && !schedule_fails(&job, &min_atoms, None, DelayBind::Defaults, target) {
        return MinifyOutcome::HarnessMismatch;
    }
    let extras = atoms_to_extras(&min_atoms);
    MinifyOutcome::Minified(MinResult {
        seed: job.input.seed,
        check: target,
        atoms_before,
        atoms_after: min_atoms.len(),
        extras_before,
        extras_after: extras.len(),
        rounds,
        capped,
        extras,
        atoms: min_atoms,
    })
}

pub fn format_min_schedule(result: &MinResult) -> String {
    let mut s = format!(
        "# seed {}\n# check {}\n# atoms {} -> {}\n# extras {} -> {}\n# rounds {}\n",
        result.seed,
        result.check.as_label(),
        result.atoms_before,
        result.atoms_after,
        result.extras_before,
        result.extras_after,
        result.rounds,
    );
    if result.capped {
        s.push_str("# capped 1\n");
    }
    for atom in &result.atoms {
        s.push_str(&format_atom(atom));
        s.push('\n');
    }
    s
}

pub fn atoms_to_extras(atoms: &[Atom]) -> Vec<(Timestamp, WorldEvent)> {
    let mut stamped = Vec::new();
    for atom in atoms {
        match atom {
            Atom::CrashWindow {
                orig,
                orig_up,
                node,
                down_at,
                up_at,
                torn,
            } => {
                stamped.push((
                    down_at.0,
                    *orig,
                    WorldEvent::Crash {
                        node: *node,
                        torn_extra: Some(*torn),
                    },
                ));
                if let Some(up) = up_at {
                    stamped.push((
                        up.0,
                        orig_up.unwrap_or(*orig),
                        WorldEvent::Recover { node: *node },
                    ));
                }
            }
            Atom::PartitionWindow {
                orig,
                orig_heal,
                from,
                to,
                cut_at,
                heal_at,
                asymmetric,
            } => {
                stamped.push((
                    cut_at.0,
                    *orig,
                    WorldEvent::Partition {
                        from: *from,
                        to: *to,
                        connected: false,
                        asymmetric: *asymmetric,
                    },
                ));
                if let Some(heal) = heal_at {
                    stamped.push((
                        heal.0,
                        orig_heal.unwrap_or(*orig),
                        WorldEvent::Partition {
                            from: *from,
                            to: *to,
                            connected: true,
                            asymmetric: *asymmetric,
                        },
                    ));
                }
            }
            Atom::PartitionHeal {
                orig,
                t,
                from,
                to,
                asymmetric,
            } => {
                stamped.push((
                    t.0,
                    *orig,
                    WorldEvent::Partition {
                        from: *from,
                        to: *to,
                        connected: true,
                        asymmetric: *asymmetric,
                    },
                ));
            }
            Atom::Client { orig, t, node, req } => {
                stamped.push((
                    t.0,
                    *orig,
                    WorldEvent::ClientInject {
                        node: *node,
                        req: req.clone(),
                    },
                ));
            }
            Atom::Recover { orig, t, node } => {
                stamped.push((t.0, *orig, WorldEvent::Recover { node: *node }));
            }
            Atom::FailNextFsync { orig, t, node } => {
                stamped.push((t.0, *orig, WorldEvent::FailNextFsync { node: *node }));
            }
            Atom::Drop(_) | Atom::Dup(_) => {}
        }
    }
    stamped.sort_by_key(|(t, orig, _)| (*t, *orig));
    stamped
        .into_iter()
        .map(|(t, _, ev)| (Timestamp(t), ev))
        .collect()
}

pub(crate) fn atomize(
    extras: &[(Timestamp, WorldEvent)],
    observed: &ObservedSchedule,
) -> Vec<Atom> {
    let mut used = vec![false; extras.len()];
    let mut atoms = Vec::new();
    for i in 0..extras.len() {
        if used[i] {
            continue;
        }
        match &extras[i].1 {
            WorldEvent::Crash { node, torn_extra } => {
                used[i] = true;
                let torn = observed
                    .crash_torn
                    .get(&(extras[i].0 .0, node.0))
                    .copied()
                    .or(*torn_extra)
                    .unwrap_or(0);
                let mut up_at = None;
                let mut orig_up = None;
                for (j, item) in extras.iter().enumerate().skip(i + 1) {
                    if used[j] {
                        continue;
                    }
                    if let WorldEvent::Recover { node: n2 } = item.1 {
                        if n2 == *node {
                            used[j] = true;
                            up_at = Some(item.0);
                            orig_up = Some(j);
                            break;
                        }
                    }
                }
                atoms.push(Atom::CrashWindow {
                    orig: i,
                    orig_up,
                    node: *node,
                    down_at: extras[i].0,
                    up_at,
                    torn,
                });
            }
            WorldEvent::Partition {
                from,
                to,
                connected: false,
                asymmetric,
            } => {
                used[i] = true;
                let mut heal_at = None;
                let mut orig_heal = None;
                for (j, item) in extras.iter().enumerate().skip(i + 1) {
                    if used[j] {
                        continue;
                    }
                    if let WorldEvent::Partition {
                        from: f2,
                        to: t2,
                        connected: true,
                        asymmetric: a2,
                    } = item.1
                    {
                        if f2 == *from && t2 == *to && a2 == *asymmetric {
                            used[j] = true;
                            heal_at = Some(item.0);
                            orig_heal = Some(j);
                            break;
                        }
                    }
                }
                atoms.push(Atom::PartitionWindow {
                    orig: i,
                    orig_heal,
                    from: *from,
                    to: *to,
                    cut_at: extras[i].0,
                    heal_at,
                    asymmetric: *asymmetric,
                });
            }
            _ => {}
        }
    }
    for (i, (t, ev)) in extras.iter().enumerate() {
        if used[i] {
            continue;
        }
        match ev {
            WorldEvent::ClientInject { node, req } => {
                atoms.push(Atom::Client {
                    orig: i,
                    t: *t,
                    node: *node,
                    req: req.clone(),
                });
            }
            WorldEvent::Recover { node } => {
                atoms.push(Atom::Recover {
                    orig: i,
                    t: *t,
                    node: *node,
                });
            }
            WorldEvent::FailNextFsync { node } => {
                atoms.push(Atom::FailNextFsync {
                    orig: i,
                    t: *t,
                    node: *node,
                });
            }
            WorldEvent::Partition {
                from,
                to,
                connected: true,
                asymmetric,
            } => {
                atoms.push(Atom::PartitionHeal {
                    orig: i,
                    t: *t,
                    from: *from,
                    to: *to,
                    asymmetric: *asymmetric,
                });
            }
            _ => {}
        }
    }
    for token in &observed.drops {
        atoms.push(Atom::Drop(*token));
    }
    for token in &observed.dups {
        atoms.push(Atom::Dup(*token));
    }
    atoms
}

fn format_atom(atom: &Atom) -> String {
    match atom {
        Atom::CrashWindow {
            node,
            down_at,
            up_at,
            torn,
            ..
        } => match up_at {
            Some(up) => format!(
                "t={} Crash node={} torn={} recover_t={}",
                down_at.0, node.0, torn, up.0
            ),
            None => format!("t={} Crash node={} torn={}", down_at.0, node.0, torn),
        },
        Atom::PartitionWindow {
            from,
            to,
            cut_at,
            heal_at,
            asymmetric,
            ..
        } => match heal_at {
            Some(heal) => format!(
                "t={} Partition {}->{} asymmetric={} heal_t={}",
                cut_at.0,
                from.0,
                to.0,
                u8::from(*asymmetric),
                heal.0
            ),
            None => format!(
                "t={} Partition {}->{} asymmetric={}",
                cut_at.0,
                from.0,
                to.0,
                u8::from(*asymmetric)
            ),
        },
        Atom::PartitionHeal {
            t,
            from,
            to,
            asymmetric,
            ..
        } => format!(
            "t={} PartitionHeal {}->{} asymmetric={}",
            t.0,
            from.0,
            to.0,
            u8::from(*asymmetric)
        ),
        Atom::Client { t, node, req, .. } => {
            format!(
                "t={} Client node={} client={} req={}",
                t.0, node.0, req.client.0, req.request.0
            )
        }
        Atom::Recover { t, node, .. } => format!("t={} Recover node={}", t.0, node.0),
        Atom::FailNextFsync { t, node, .. } => {
            format!("t={} FailNextFsync node={}", t.0, node.0)
        }
        Atom::Drop(tok) => format!(
            "drop from={} to={} hash={}",
            tok.from.0,
            tok.to.0,
            digest_hex(&tok.hash)
        ),
        Atom::Dup(tok) => format!(
            "dup from={} to={} hash={}",
            tok.from.0,
            tok.to.0,
            digest_hex(&tok.hash)
        ),
    }
}

fn delta_debug(
    job: &MinifyJob,
    atoms: Vec<Atom>,
    target: CheckName,
    rounds: &mut u32,
) -> (Vec<Atom>, bool) {
    delta_debug_with(atoms, MAX_CANDIDATE_DRAINS, rounds, |cand| {
        schedule_fails(job, cand, None, DelayBind::Defaults, target)
    })
}

fn delta_debug_with<F>(
    mut atoms: Vec<Atom>,
    budget: u32,
    rounds: &mut u32,
    mut fails: F,
) -> (Vec<Atom>, bool)
where
    F: FnMut(&[Atom]) -> bool,
{
    loop {
        if atoms.len() < 2 {
            break;
        }
        if *rounds >= budget {
            return (atoms, true);
        }
        let mid = atoms.len() / 2;
        let right = atoms[mid..].to_vec();
        if propose(budget, rounds, &right, &mut fails) {
            atoms = right;
            continue;
        }
        let left = atoms[..mid].to_vec();
        if propose(budget, rounds, &left, &mut fails) {
            atoms = left;
            continue;
        }
        break;
    }
    let mut changed = true;
    while changed {
        changed = false;
        let mut i = 0;
        while i < atoms.len() {
            if *rounds >= budget {
                return (atoms, true);
            }
            let mut cand = atoms.clone();
            cand.remove(i);
            if propose(budget, rounds, &cand, &mut fails) {
                atoms = cand;
                changed = true;
            } else {
                i = i.saturating_add(1);
            }
        }
    }
    (atoms, false)
}

fn propose<F>(budget: u32, rounds: &mut u32, atoms: &[Atom], fails: &mut F) -> bool
where
    F: FnMut(&[Atom]) -> bool,
{
    if *rounds >= budget {
        return false;
    }
    *rounds = rounds.saturating_add(1);
    fails(atoms)
}

fn schedule_fails(
    job: &MinifyJob,
    atoms: &[Atom],
    book: Option<&ReplayBook>,
    bind: DelayBind,
    target: CheckName,
) -> bool {
    let report = run_schedule(job, atoms, book, bind);
    matches!(&report.check, Some(fail) if fail.check == target && !fail.check.is_abort())
}

fn run_schedule(
    job: &MinifyJob,
    atoms: &[Atom],
    book: Option<&ReplayBook>,
    bind: DelayBind,
) -> RunReport {
    let extras = atoms_to_extras(atoms);
    let drops: Vec<DeliveryToken> = atoms
        .iter()
        .filter_map(|a| match a {
            Atom::Drop(t) => Some(*t),
            _ => None,
        })
        .collect();
    let dups: Vec<DeliveryToken> = atoms
        .iter()
        .filter_map(|a| match a {
            Atom::Dup(t) => Some(*t),
            _ => None,
        })
        .collect();
    let book = match bind {
        DelayBind::Recorded => book.cloned().unwrap_or_default(),
        DelayBind::Defaults => ReplayBook::default(),
    };
    let mut cluster = Cluster::new(job.input.seed, job.input.cfg.clone());
    cluster.use_schedule_replay(book, drops, dups, bind);
    for (id, yes) in &job.skip_vote_persist {
        cluster.set_skip_vote_persist(*id, *yes);
    }
    for i in 0..job.input.cfg.n {
        cluster.inject_recover(NodeId(i));
    }
    for (at, event) in &extras {
        cluster.inject_at(*at, event.clone());
    }
    cluster.drain_horizon();
    finish_report(
        job.input.seed,
        job.input.profile,
        job.input.cfg.clone(),
        extras,
        &cluster,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronos_protocol::{ClientId, ClientReq, Cmd, RequestId};

    fn junk_client(t: u64, req: u64) -> (Timestamp, WorldEvent) {
        (
            Timestamp(t),
            WorldEvent::ClientInject {
                node: NodeId(0),
                req: ClientReq {
                    client: ClientId(1),
                    request: RequestId(req),
                    cmd: Cmd::Get { key: b"k".to_vec() },
                },
            },
        )
    }

    fn planted_cfg() -> SimConfig {
        SimConfig {
            n: 3,
            jitter_max_ns: 0,
            io_delay_min_ns: 0,
            io_delay_max_ns: 0,
            net_delay_min_ns: 0,
            net_delay_max_ns: 0,
            ..SimConfig::default()
        }
    }

    fn public_input(extras: Vec<(Timestamp, WorldEvent)>) -> MinifyInput {
        MinifyInput {
            seed: 3,
            profile: Profile::Calm,
            cfg: planted_cfg(),
            extras,
        }
    }

    fn token(n: u8) -> DeliveryToken {
        DeliveryToken {
            from: NodeId(0),
            to: NodeId(1),
            hash: [n; 32],
        }
    }

    #[test]
    fn atomize_pairs_crash_and_recover() {
        let extras = vec![
            (
                Timestamp(50),
                WorldEvent::Crash {
                    node: NodeId(0),
                    torn_extra: None,
                },
            ),
            (Timestamp(100), WorldEvent::Recover { node: NodeId(0) }),
            junk_client(200, 1),
        ];
        let mut observed = ObservedSchedule::default();
        observed.crash_torn.insert((50, 0), 3);
        let atoms = atomize(&extras, &observed);
        assert_eq!(atoms.len(), 2);
        match &atoms[0] {
            Atom::CrashWindow {
                node,
                down_at,
                up_at,
                torn,
                orig,
                orig_up,
            } => {
                assert_eq!(*node, NodeId(0));
                assert_eq!(down_at.0, 50);
                assert_eq!(up_at.map(|t| t.0), Some(100));
                assert_eq!(*torn, 3);
                assert_eq!(*orig, 0);
                assert_eq!(*orig_up, Some(1));
            }
            other => panic!("expected crash window, got {other:?}"),
        }
        assert!(matches!(atoms[1], Atom::Client { .. }));
        let back = atoms_to_extras(&atoms);
        assert_eq!(back.len(), 3);
        match &back[0].1 {
            WorldEvent::Crash {
                torn_extra: Some(3),
                ..
            } => {}
            other => panic!("torn not pinned: {other:?}"),
        }
    }

    #[test]
    fn atomize_unpaired_heal_stays_a_heal() {
        let extras = vec![(
            Timestamp(10),
            WorldEvent::Partition {
                from: NodeId(0),
                to: NodeId(1),
                connected: true,
                asymmetric: false,
            },
        )];
        let atoms = atomize(&extras, &ObservedSchedule::default());
        match &atoms[0] {
            Atom::PartitionHeal {
                from, to, t, orig, ..
            } => {
                assert_eq!(*from, NodeId(0));
                assert_eq!(*to, NodeId(1));
                assert_eq!(t.0, 10);
                assert_eq!(*orig, 0);
            }
            other => panic!("expected PartitionHeal, got {other:?}"),
        }
        let back = atoms_to_extras(&atoms);
        match &back[0].1 {
            WorldEvent::Partition {
                connected: true, ..
            } => {}
            other => panic!("heal inverted to cut: {other:?}"),
        }
    }

    #[test]
    fn atoms_to_extras_keeps_same_time_orig_order() {
        let extras = vec![junk_client(100, 1), junk_client(100, 2)];
        let atoms = atomize(&extras, &ObservedSchedule::default());
        let shuffled = vec![atoms[1].clone(), atoms[0].clone()];
        let back = atoms_to_extras(&shuffled);
        match (&back[0].1, &back[1].1) {
            (WorldEvent::ClientInject { req: a, .. }, WorldEvent::ClientInject { req: b, .. }) => {
                assert_eq!(a.request.0, 1);
                assert_eq!(b.request.0, 2);
            }
            other => panic!("expected two clients: {other:?}"),
        }
    }

    #[test]
    fn format_min_schedule_names_check_and_counts() {
        let result = MinResult {
            seed: 7,
            check: CheckName::ElectionSafety,
            atoms_before: 4,
            atoms_after: 1,
            extras_before: 3,
            extras_after: 0,
            rounds: 5,
            capped: false,
            atoms: vec![Atom::Drop(token(0xab))],
            extras: Vec::new(),
        };
        let text = format_min_schedule(&result);
        assert!(text.contains("# seed 7"));
        assert!(text.contains("# check ElectionSafety"));
        assert!(text.contains("# atoms 4 -> 1"));
        assert!(text.contains("drop from=0 to=1 hash="));
        assert!(!text.contains("FAIL"));
    }

    #[test]
    fn minify_clean_seed_is_clean() {
        let outcome = minify_input(MinifyInput {
            seed: 1,
            profile: Profile::Calm,
            cfg: planted_cfg(),
            extras: Vec::new(),
        });
        assert_eq!(outcome, MinifyOutcome::Clean);
    }

    #[test]
    fn public_minify_never_plants_the_hook() {
        let outcome = minify_input(public_input(Vec::new()));
        assert_eq!(
            outcome,
            MinifyOutcome::Clean,
            "hook off: recover-only must not fail PersistBeforeSend"
        );
        let planted = minify_planted(public_input(Vec::new()), vec![(NodeId(0), true)]);
        let MinifyOutcome::Minified(result) = planted else {
            panic!("planted recover-only must fail, got {planted:?}");
        };
        assert_eq!(result.check, CheckName::PersistBeforeSend);
    }

    #[test]
    fn minify_strips_junk_extras_from_planted_persist_before_send() {
        let extras = vec![
            junk_client(50_000_000, 1),
            junk_client(80_000_000, 2),
            (
                Timestamp(10_000_000),
                WorldEvent::Partition {
                    from: NodeId(1),
                    to: NodeId(2),
                    connected: false,
                    asymmetric: false,
                },
            ),
            (
                Timestamp(20_000_000),
                WorldEvent::Partition {
                    from: NodeId(1),
                    to: NodeId(2),
                    connected: true,
                    asymmetric: false,
                },
            ),
        ];
        let outcome = minify_planted(public_input(extras), vec![(NodeId(0), true)]);
        let MinifyOutcome::Minified(result) = outcome else {
            panic!("expected minified, got {outcome:?}");
        };
        assert_eq!(result.check, CheckName::PersistBeforeSend);
        assert!(result.atoms_after <= result.atoms_before);
        assert!(
            result
                .atoms
                .iter()
                .all(|a| !matches!(a, Atom::Client { .. })),
            "junk clients should be removable: {:?}",
            result.atoms
        );
        assert!(result.extras_after <= result.extras_before);
        assert!(!result.capped);
    }

    #[test]
    fn minify_cli_path_never_sets_the_hook() {
        let outcome = minify(1);
        assert_eq!(
            outcome,
            MinifyOutcome::Clean,
            "CLI minify(1) must not plant skip_vote_persist"
        );
    }

    #[test]
    fn delta_debug_keeps_only_the_causal_atom() {
        let atoms = vec![
            Atom::Client {
                orig: 0,
                t: Timestamp(1),
                node: NodeId(0),
                req: ClientReq {
                    client: ClientId(1),
                    request: RequestId(1),
                    cmd: Cmd::Get { key: b"k".to_vec() },
                },
            },
            Atom::Drop(token(1)),
            Atom::Client {
                orig: 2,
                t: Timestamp(3),
                node: NodeId(0),
                req: ClientReq {
                    client: ClientId(1),
                    request: RequestId(2),
                    cmd: Cmd::Get { key: b"k".to_vec() },
                },
            },
        ];
        let mut rounds = 0u32;
        let (kept, capped) = delta_debug_with(atoms, 100, &mut rounds, |cand| {
            cand.iter().any(|a| matches!(a, Atom::Drop(_)))
        });
        assert!(!capped);
        assert_eq!(kept.len(), 1);
        assert!(matches!(kept[0], Atom::Drop(_)));
        assert!(rounds > 0);
    }

    #[test]
    fn search_cap_does_not_become_mismatch() {
        let atoms = vec![Atom::Drop(token(1)), Atom::Dup(token(2))];
        let mut rounds = 0u32;
        let (kept, capped) = delta_debug_with(atoms.clone(), 0, &mut rounds, |_| {
            panic!("cap must not propose candidates")
        });
        assert!(capped);
        assert_eq!(kept, atoms);
        assert_eq!(rounds, 0);
    }

    #[test]
    fn full_observed_schedule_reproduces_swarm_check() {
        let seed = 7u64;
        let plan = swarm_plan(seed);
        let swarm = drain_plan(seed, &plan.cfg, &plan.extras, &[]);
        let swarm_check = swarm.check_fail().map(|f| f.check);
        let atoms = atomize(&plan.extras, swarm.observed());
        let job = MinifyJob {
            input: MinifyInput {
                seed,
                profile: plan.profile,
                cfg: plan.cfg,
                extras: plan.extras,
            },
            skip_vote_persist: Vec::new(),
        };
        let report = run_schedule(
            &job,
            &atoms,
            Some(&swarm.observed().book),
            DelayBind::Recorded,
        );
        assert_eq!(
            report.check.as_ref().map(|f| f.check),
            swarm_check,
            "seed {seed} full schedule must match swarm CheckName"
        );
    }

    #[test]
    fn dropping_the_causal_atom_clears_the_oracle() {
        let causal = Atom::FailNextFsync {
            orig: 0,
            t: Timestamp(1),
            node: NodeId(0),
        };
        let junk = Atom::Client {
            orig: 1,
            t: Timestamp(2),
            node: NodeId(0),
            req: ClientReq {
                client: ClientId(1),
                request: RequestId(1),
                cmd: Cmd::Get { key: b"k".to_vec() },
            },
        };
        let full = vec![junk.clone(), causal.clone()];
        let fails = |atoms: &[Atom]| {
            atoms
                .iter()
                .any(|a| matches!(a, Atom::FailNextFsync { .. }))
        };
        assert!(fails(&full));
        let without: Vec<Atom> = full
            .iter()
            .filter(|a| !matches!(a, Atom::FailNextFsync { .. }))
            .cloned()
            .collect();
        assert!(!fails(&without));
        let mut rounds = 0u32;
        let (kept, _) = delta_debug_with(full, 100, &mut rounds, fails);
        assert_eq!(kept, vec![causal]);
    }
}
