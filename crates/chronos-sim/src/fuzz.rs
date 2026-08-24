//! Swarm: seed → planned extras + SimConfig → Cluster drain.
//!
//! Two `Rng::new(seed)` streams: planner vs world. `run()` stays the G3 oracle.
//! Spec: `docs/02-architecture.md` § Fuzzing and minimization.

use std::collections::BTreeMap;

use chronos_protocol::codec::{put_u32_le, put_u64_le};
use chronos_protocol::{ClientId, ClientReq, Cmd, NodeId, RequestId, Timestamp};

use crate::check::{CheckFail, CheckName};
use crate::cluster::{Cluster, SimConfig};
use crate::rng::Rng;
use crate::scheduler::WorldEvent;
use crate::trace::digest_hex;

const MAX_CLIENT_OPS: usize = 40;
const MAX_EXTRAS: usize = 64;
const ELECTION_MIN_NS: u64 = 150_000_000;
const ELECTION_MAX_NS: u64 = 300_000_000;
const HEARTBEAT_NS: u64 = 50_000_000;
const SLOW_FSYNC_EXTRA_NS: u64 = 50_000_000;

/// Fault knobs for a run. Default is P3-safe: lie off, ppm 0, buggify off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaultConfig {
    pub drop_ppm: u32,
    pub dup_ppm: u32,
    pub torn_suffix: bool,
    pub fsync_ok_but_not_durable: bool,
    pub buggify_slow_fsync: bool,
    pub buggify_fsync_extra_ns: u64,
    pub buggify_election_edge_max: bool,
    pub buggify_reject_ok_ae: bool,
}

impl Default for FaultConfig {
    fn default() -> Self {
        Self {
            drop_ppm: 0,
            dup_ppm: 0,
            torn_suffix: true,
            fsync_ok_but_not_durable: false,
            buggify_slow_fsync: false,
            buggify_fsync_extra_ns: 0,
            buggify_election_edge_max: false,
            buggify_reject_ok_ae: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Calm,
    Brutal,
}

impl Profile {
    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Calm => "calm",
            Profile::Brutal => "brutal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwarmPlan {
    pub profile: Profile,
    pub cfg: SimConfig,
    pub extras: Vec<(Timestamp, WorldEvent)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub n: u8,
    pub brutal: bool,
    pub crash: bool,
    pub partition: bool,
    pub fsync_err: bool,
    pub drop: bool,
    pub dup: bool,
    pub buggify: bool,
    pub torn: bool,
}

/// Batch fold of per-run [`Coverage`].
/// Profile knobs (n / brutal / buggify) are separate from observed fault fires.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoverageSummary {
    pub runs: u64,
    pub profile: BTreeMap<&'static str, u64>,
    pub observed: BTreeMap<&'static str, u64>,
    pub pairs: BTreeMap<(&'static str, &'static str), u64>,
}

const COVERAGE_TOP_PAIRS: usize = 16;

/// Run profile / config knobs (not proof the weird path executed).
pub fn coverage_profile_flags(c: &Coverage) -> Vec<&'static str> {
    let mut flags = Vec::new();
    match c.n {
        3 => flags.push("n3"),
        5 => flags.push("n5"),
        _ => flags.push("n_other"),
    }
    if c.brutal {
        flags.push("brutal");
    }
    if c.buggify {
        flags.push("buggify");
    }
    flags
}

/// Faults that actually showed up on the run (extras applied or cluster-observed).
pub fn coverage_observed_flags(c: &Coverage) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if c.crash {
        flags.push("crash");
    }
    if c.partition {
        flags.push("partition");
    }
    if c.fsync_err {
        flags.push("fsync_err");
    }
    if c.drop {
        flags.push("drop");
    }
    if c.dup {
        flags.push("dup");
    }
    if c.torn {
        flags.push("torn");
    }
    flags
}

/// All flags for one run (profile then observed). Prefer the split helpers for new code.
pub fn coverage_flags(c: &Coverage) -> Vec<&'static str> {
    let mut flags = coverage_profile_flags(c);
    flags.extend(coverage_observed_flags(c));
    flags
}

pub fn aggregate_coverage<'a, I>(reports: I) -> CoverageSummary
where
    I: IntoIterator<Item = &'a Coverage>,
{
    let mut summary = CoverageSummary::default();
    for c in reports {
        summary.runs = summary.runs.saturating_add(1);
        for &f in &coverage_profile_flags(c) {
            let e = summary.profile.entry(f).or_insert(0);
            *e = e.saturating_add(1);
        }
        let observed = coverage_observed_flags(c);
        for &f in &observed {
            let e = summary.observed.entry(f).or_insert(0);
            *e = e.saturating_add(1);
        }
        for i in 0..observed.len() {
            for j in (i + 1)..observed.len() {
                let a = observed[i];
                let b = observed[j];
                let key = if a <= b { (a, b) } else { (b, a) };
                let e = summary.pairs.entry(key).or_insert(0);
                *e = e.saturating_add(1);
            }
        }
    }
    summary
}

pub fn format_coverage_table(summary: &CoverageSummary) -> String {
    let mut s = format!(
        "# coverage runs={}\n# claim: profile=run knobs; observed=faults that fired; not all combinations\n",
        summary.runs
    );
    s.push_str("# profile\n");
    if summary.profile.is_empty() {
        s.push_str("(none)\n");
    } else {
        for (flag, count) in &summary.profile {
            s.push_str(&format!("{flag} {count}\n"));
        }
    }
    s.push_str("# observed\n");
    if summary.observed.is_empty() {
        s.push_str("(none)\n");
    } else {
        for (flag, count) in &summary.observed {
            s.push_str(&format!("{flag} {count}\n"));
        }
    }
    s.push_str("# top pairs (observed same-run co-occurrence; not causal nesting)\n");
    let mut pairs: Vec<_> = summary.pairs.iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    if pairs.is_empty() {
        s.push_str("(none)\n");
    } else {
        for ((a, b), count) in pairs.into_iter().take(COVERAGE_TOP_PAIRS) {
            s.push_str(&format!("{a}+{b} {count}\n"));
        }
    }
    s
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    pub seed: u64,
    pub profile: Profile,
    pub cfg: SimConfig,
    pub extras: Vec<(Timestamp, WorldEvent)>,
    pub digest: [u8; 32],
    pub check: Option<CheckFail>,
    pub encoded_trace: Vec<u8>,
    pub counters: Coverage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailFileHeader {
    pub seed: u64,
    pub digest: [u8; 32],
    pub check: Option<CheckName>,
    pub config: Vec<u8>,
    pub extras: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayVerdict {
    Reproduced,
    Clean,
    DidNotReproduce,
    DigestMismatch,
    CheckMismatch,
    ConfigMismatch,
    ExtrasMismatch,
}

impl ReplayVerdict {
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Clean | Self::DidNotReproduce => 0,
            Self::Reproduced => 1,
            Self::DigestMismatch
            | Self::CheckMismatch
            | Self::ConfigMismatch
            | Self::ExtrasMismatch => 2,
        }
    }
}

pub fn encode_config(cfg: &SimConfig) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(cfg.n);
    put_u64_le(&mut buf, cfg.election_min_ns);
    put_u64_le(&mut buf, cfg.election_max_ns);
    put_u64_le(&mut buf, cfg.heartbeat_ns);
    put_u64_le(&mut buf, cfg.io_delay_min_ns);
    put_u64_le(&mut buf, cfg.io_delay_max_ns);
    put_u64_le(&mut buf, cfg.net_delay_min_ns);
    put_u64_le(&mut buf, cfg.net_delay_max_ns);
    put_u64_le(&mut buf, cfg.jitter_max_ns);
    put_u64_le(&mut buf, cfg.max_ns);
    put_u32_le(&mut buf, cfg.drop_ppm);
    put_u32_le(&mut buf, cfg.dup_ppm);
    buf.push(u8::from(cfg.torn_suffix));
    buf.push(u8::from(cfg.fsync_ok_but_not_durable));
    buf.push(u8::from(cfg.buggify_slow_fsync));
    put_u64_le(&mut buf, cfg.buggify_fsync_extra_ns);
    buf.push(u8::from(cfg.buggify_election_edge_max));
    buf.push(u8::from(cfg.buggify_reject_ok_ae));
    buf.push(u8::from(cfg.record_roles));
    buf.push(u8::from(cfg.check_safety));
    buf.push(u8::from(cfg.check_engineering));
    buf.push(u8::from(cfg.check_liveness));
    buf
}

pub fn swarm_plan(seed: u64) -> SwarmPlan {
    let mut rng = Rng::new(seed);
    let profile = if rng.bool(500_000) {
        Profile::Brutal
    } else {
        Profile::Calm
    };
    let n = if rng.bool(500_000) { 5 } else { 3 };
    let (io_max, net_max, jitter_max, max_ns, drop_lo, drop_hi, dup_hi) = match profile {
        Profile::Calm => (5_000_000, 5_000_000, 1_000_000, 2_000_000_000, 0, 1_000, 0),
        Profile::Brutal => (
            50_000_000,
            80_000_000,
            5_000_000,
            5_000_000_000,
            1_000,
            50_000,
            10_000,
        ),
    };
    let drop_ppm = rng.delay_ns(drop_lo, drop_hi) as u32;
    let dup_ppm = rng.delay_ns(0, dup_hi) as u32;
    let slow = profile == Profile::Brutal && rng.bool(500_000);
    let edge = profile == Profile::Brutal && rng.bool(500_000);
    let reject = profile == Profile::Brutal && rng.bool(250_000);
    let cfg = SimConfig {
        n,
        election_min_ns: ELECTION_MIN_NS,
        election_max_ns: ELECTION_MAX_NS,
        heartbeat_ns: HEARTBEAT_NS,
        io_delay_min_ns: 0,
        io_delay_max_ns: io_max,
        net_delay_min_ns: 0,
        net_delay_max_ns: net_max,
        jitter_max_ns: jitter_max,
        max_ns,
        drop_ppm,
        dup_ppm,
        torn_suffix: true,
        fsync_ok_but_not_durable: false,
        buggify_slow_fsync: slow,
        buggify_fsync_extra_ns: if slow { SLOW_FSYNC_EXTRA_NS } else { 0 },
        buggify_election_edge_max: edge,
        buggify_reject_ok_ae: reject,
        record_roles: false,
        check_safety: true,
        check_engineering: true,
        check_liveness: false,
    };
    let extras = plan_extras(&mut rng, profile, &cfg);
    SwarmPlan {
        profile,
        cfg,
        extras,
    }
}

pub fn run_seed(seed: u64) -> RunReport {
    let plan = swarm_plan(seed);
    run_plan(seed, plan.profile, plan.cfg, plan.extras)
}

/// Drain `extras` under the swarm (probabilistic) policy. Same body as `run_seed`
/// after `swarm_plan`.
pub fn run_plan(
    seed: u64,
    profile: Profile,
    cfg: SimConfig,
    extras: Vec<(Timestamp, WorldEvent)>,
) -> RunReport {
    let cluster = drain_plan(seed, &cfg, &extras, &[]);
    finish_report(seed, profile, cfg, extras, &cluster)
}

pub(crate) fn drain_plan(
    seed: u64,
    cfg: &SimConfig,
    extras: &[(Timestamp, WorldEvent)],
    skip_vote_persist: &[(NodeId, bool)],
) -> Cluster {
    let mut cluster = Cluster::new(seed, cfg.clone());
    for (id, yes) in skip_vote_persist {
        cluster.set_skip_vote_persist(*id, *yes);
    }
    for i in 0..cfg.n {
        cluster.inject_recover(NodeId(i));
    }
    for (at, event) in extras {
        cluster.inject_at(*at, event.clone());
    }
    cluster.drain_horizon();
    cluster
}

pub(crate) fn finish_report(
    seed: u64,
    profile: Profile,
    cfg: SimConfig,
    extras: Vec<(Timestamp, WorldEvent)>,
    cluster: &Cluster,
) -> RunReport {
    let counters = Coverage {
        n: cfg.n,
        brutal: profile == Profile::Brutal,
        crash: extras
            .iter()
            .any(|(_, e)| matches!(e, WorldEvent::Crash { .. })),
        partition: extras.iter().any(|(_, e)| {
            matches!(
                e,
                WorldEvent::Partition {
                    connected: false,
                    ..
                }
            )
        }),
        fsync_err: extras
            .iter()
            .any(|(_, e)| matches!(e, WorldEvent::FailNextFsync { .. })),
        drop: cluster
            .dropped()
            .iter()
            .any(|d| d.3 == crate::scheduler::DropReason::Loss),
        dup: cluster.dup_sends() > 0,
        buggify: cfg.buggify_slow_fsync
            || cfg.buggify_election_edge_max
            || cfg.buggify_reject_ok_ae,
        torn: cluster.torn_applied(),
    };
    RunReport {
        seed,
        profile,
        cfg,
        extras,
        digest: cluster.digest(),
        check: cluster.check_fail().cloned(),
        encoded_trace: cluster.encoded_trace(),
        counters,
    }
}

pub fn format_fail_file(report: &RunReport) -> Vec<u8> {
    let check = check_label(report);
    let detail = report
        .check
        .as_ref()
        .map(|f| f.detail.replace(['\n', '\r'], " "))
        .unwrap_or_default();
    let header = format!(
        "chronos-fail 1\nseed {}\nprofile {}\ndigest {}\ncheck {}\ndetail {}\nconfig {}\nextras {}\n--\n",
        report.seed,
        report.profile.as_str(),
        digest_hex(&report.digest),
        check,
        detail,
        hex_encode(&encode_config(&report.cfg)),
        hex_encode(&encode_extras(&report.extras)),
    );
    let mut out = header.into_bytes();
    out.extend_from_slice(&report.encoded_trace);
    out
}

pub fn seed_from_fail_file(bytes: &[u8]) -> Option<u64> {
    fail_file_header(bytes).map(|h| h.seed)
}

pub fn fail_file_header(bytes: &[u8]) -> Option<FailFileHeader> {
    let header = fail_header_text(bytes)?;
    let mut seed = None;
    let mut digest = None;
    let mut check = None;
    let mut check_seen = false;
    let mut config = None;
    let mut extras = None;
    for line in header.lines() {
        if let Some(rest) = line.strip_prefix("seed ") {
            seed = Some(rest.parse().ok()?);
        } else if let Some(rest) = line.strip_prefix("digest ") {
            digest = Some(parse_digest_hex(rest.trim())?);
        } else if let Some(rest) = line.strip_prefix("check ") {
            let label = rest.trim();
            check_seen = true;
            check = if label == "none" {
                None
            } else {
                Some(CheckName::from_label(label)?)
            };
        } else if let Some(rest) = line.strip_prefix("config ") {
            config = Some(parse_hex(rest.trim())?);
        } else if let Some(rest) = line.strip_prefix("extras ") {
            extras = Some(parse_hex(rest.trim())?);
        }
    }
    if !check_seen {
        return None;
    }
    Some(FailFileHeader {
        seed: seed?,
        digest: digest?,
        check,
        config: config?,
        extras: extras?,
    })
}

pub fn verify_replay(header: &FailFileHeader, report: &RunReport) -> ReplayVerdict {
    if report.check.is_none() {
        return if header.check.is_none() {
            ReplayVerdict::Clean
        } else {
            ReplayVerdict::DidNotReproduce
        };
    }
    if report.digest != header.digest {
        return ReplayVerdict::DigestMismatch;
    }
    if report.check.as_ref().map(|f| f.check) != header.check {
        return ReplayVerdict::CheckMismatch;
    }
    if encode_config(&report.cfg) != header.config {
        return ReplayVerdict::ConfigMismatch;
    }
    if encode_extras(&report.extras) != header.extras {
        return ReplayVerdict::ExtrasMismatch;
    }
    ReplayVerdict::Reproduced
}

pub fn format_replay_line(
    verdict: ReplayVerdict,
    header: &FailFileHeader,
    report: &RunReport,
) -> String {
    let got_digest = digest_hex(&report.digest);
    let header_digest = digest_hex(&header.digest);
    let header_check = check_label_of(header.check);
    let got_check = check_label(report);
    match verdict {
        ReplayVerdict::Reproduced => format!(
            "REPRODUCED seed={} digest={got_digest} check={got_check}",
            report.seed
        ),
        ReplayVerdict::Clean => {
            format!("CLEAN seed={} digest={got_digest}", report.seed)
        }
        ReplayVerdict::DidNotReproduce => format!(
            "DID_NOT_REPRODUCE seed={} header_check={header_check} digest={got_digest}",
            report.seed
        ),
        ReplayVerdict::DigestMismatch => {
            format!("MISMATCH digest header={header_digest} got={got_digest}")
        }
        ReplayVerdict::CheckMismatch => {
            format!("MISMATCH check header={header_check} got={got_check}")
        }
        ReplayVerdict::ConfigMismatch => "MISMATCH config".into(),
        ReplayVerdict::ExtrasMismatch => "MISMATCH extras".into(),
    }
}

pub fn format_planned_schedule(extras: &[(Timestamp, WorldEvent)]) -> String {
    let mut s = String::new();
    for (t, ev) in extras {
        s.push_str(&format!("t={} {:?}\n", t.0, ev));
    }
    s
}

fn check_label(report: &RunReport) -> String {
    check_label_of(report.check.as_ref().map(|f| f.check))
}

fn check_label_of(check: Option<CheckName>) -> String {
    check.map(CheckName::as_label).unwrap_or("none").to_string()
}

fn fail_header_text(bytes: &[u8]) -> Option<String> {
    let mut i = 0;
    while i < bytes.len() {
        let rel = bytes[i..].iter().position(|&b| b == b'\n' || b == b'\r')?;
        let eol = i + rel;
        let next = if bytes[eol] == b'\r' && eol + 1 < bytes.len() && bytes[eol + 1] == b'\n' {
            eol + 2
        } else {
            eol + 1
        };
        if &bytes[i..eol] == b"--" {
            let header = std::str::from_utf8(&bytes[..i]).ok()?;
            return Some(header.replace("\r\n", "\n").replace('\r', "\n"));
        }
        i = next;
    }
    None
}

fn parse_digest_hex(s: &str) -> Option<[u8; 32]> {
    let bytes = parse_hex(s)?;
    bytes.try_into().ok()
}

fn parse_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().as_chunks::<2>().0 {
        let byte = std::str::from_utf8(chunk).ok()?;
        out.push(u8::from_str_radix(byte, 16).ok()?);
    }
    Some(out)
}

fn plan_extras(rng: &mut Rng, profile: Profile, cfg: &SimConfig) -> Vec<(Timestamp, WorldEvent)> {
    let mut extras = Vec::new();
    let mut downs: Vec<(NodeId, u64, u64)> = Vec::new();
    let n_crash = match profile {
        Profile::Calm => rng.delay_ns(0, 1),
        Profile::Brutal => rng.delay_ns(1, 3),
    };
    for _ in 0..n_crash {
        if extras.len() + 2 > MAX_EXTRAS {
            break;
        }
        let delay = rng.delay_ns(cfg.election_max_ns, cfg.election_max_ns.saturating_mul(2));
        let Some(at) = pick_time_from(rng, 1, cfg.max_ns.saturating_sub(delay).saturating_add(1))
        else {
            continue;
        };
        let recover_at = Timestamp(at.0.saturating_add(delay));
        let Some(node) = pick_free_window(rng, cfg.n, &downs, at.0, recover_at.0) else {
            continue;
        };
        downs.push((node, at.0, recover_at.0.max(at.0.saturating_add(1))));
        extras.push((
            at,
            WorldEvent::Crash {
                node,
                torn_extra: None,
            },
        ));
        extras.push((recover_at, WorldEvent::Recover { node }));
    }

    let n_part = match profile {
        Profile::Calm => rng.delay_ns(0, 1),
        Profile::Brutal => rng.delay_ns(1, 2),
    };
    for _ in 0..n_part {
        if extras.len() + 2 > MAX_EXTRAS {
            break;
        }
        if cfg.n < 2 {
            break;
        }
        let hold = match profile {
            Profile::Calm => cfg.election_max_ns,
            Profile::Brutal => cfg.election_max_ns.saturating_mul(rng.delay_ns(3, 8)),
        };
        let Some(at) = pick_time_from(rng, 1, cfg.max_ns.saturating_sub(hold).saturating_add(1))
        else {
            continue;
        };
        let heal_at = Timestamp(at.0.saturating_add(hold));
        let from = NodeId(pick_idx(rng, usize::from(cfg.n)) as u8);
        let mut to = NodeId(pick_idx(rng, usize::from(cfg.n)) as u8);
        if to == from {
            to = NodeId((from.0 + 1) % cfg.n);
        }
        let asymmetric = profile == Profile::Brutal && rng.bool(200_000);
        extras.push((
            at,
            WorldEvent::Partition {
                from,
                to,
                connected: false,
                asymmetric,
            },
        ));
        extras.push((
            heal_at,
            WorldEvent::Partition {
                from,
                to,
                connected: true,
                asymmetric,
            },
        ));
    }

    let n_fsync = match profile {
        Profile::Calm => 0,
        Profile::Brutal => rng.delay_ns(0, 2),
    };
    for _ in 0..n_fsync {
        if extras.len() + 1 > MAX_EXTRAS {
            break;
        }
        let Some(at) = pick_time(rng, cfg.max_ns) else {
            break;
        };
        let Some(node) = pick_live(rng, cfg.n, &downs, at) else {
            continue;
        };
        extras.push((at, WorldEvent::FailNextFsync { node }));
    }

    let n_ops = match profile {
        Profile::Calm => rng.delay_ns(8, 16),
        Profile::Brutal => rng.delay_ns(16, 32),
    } as usize;
    let n_ops = n_ops.min(MAX_CLIENT_OPS);
    let mut next_req = [0u64, 1, 1];
    let mut last_client: Option<(Timestamp, WorldEvent)> = None;
    for _ in 0..n_ops {
        if extras.len() + 1 > MAX_EXTRAS {
            break;
        }
        let Some(at) = pick_time(rng, cfg.max_ns) else {
            break;
        };
        let Some(node) = pick_live(rng, cfg.n, &downs, at) else {
            continue;
        };
        let client_i = if rng.bool(500_000) { 2 } else { 1 };
        let request = next_req[client_i];
        next_req[client_i] = request.saturating_add(1);
        let key = if rng.bool(500_000) {
            b"k".to_vec()
        } else {
            b"m".to_vec()
        };
        let cmd = if rng.bool(250_000) {
            Cmd::Get { key }
        } else {
            Cmd::Put {
                key,
                value: if rng.bool(500_000) {
                    b"a".to_vec()
                } else {
                    b"b".to_vec()
                },
            }
        };
        let ev = WorldEvent::ClientInject {
            node,
            req: ClientReq {
                client: ClientId(client_i as u64),
                request: RequestId(request),
                cmd,
            },
        };
        extras.push((at, ev.clone()));
        last_client = Some((at, ev));
    }
    if profile == Profile::Brutal && rng.bool(50_000) {
        if let Some((at, ev)) = last_client {
            if extras.len() < MAX_EXTRAS {
                if let Some(retry_at) = pick_time_from(rng, at.0.saturating_add(1), cfg.max_ns) {
                    extras.push((retry_at, ev));
                }
            }
        }
    }
    debug_assert!(timestamps_within(&extras, cfg.max_ns));
    debug_assert!(crash_windows_disjoint(&extras));
    extras
}

fn pick_time(rng: &mut Rng, max_ns: u64) -> Option<Timestamp> {
    pick_time_from(rng, 1, max_ns)
}

fn pick_time_from(rng: &mut Rng, min_ns: u64, max_ns: u64) -> Option<Timestamp> {
    let hi = max_ns.saturating_sub(1);
    if min_ns > hi {
        return None;
    }
    Some(Timestamp(rng.delay_ns(min_ns, hi)))
}

fn pick_idx(rng: &mut Rng, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    rng.delay_ns(0, (n as u64).saturating_sub(1)) as usize
}

fn timestamps_within(extras: &[(Timestamp, WorldEvent)], max_ns: u64) -> bool {
    extras.iter().all(|(t, _)| t.0 <= max_ns)
}

fn crash_windows_disjoint(extras: &[(Timestamp, WorldEvent)]) -> bool {
    let mut crashes = Vec::new();
    let mut recovers = Vec::new();
    for (t, ev) in extras {
        match ev {
            WorldEvent::Crash { node, .. } => crashes.push((*node, t.0)),
            WorldEvent::Recover { node } => recovers.push((*node, t.0)),
            _ => {}
        }
    }
    let mut used = vec![false; recovers.len()];
    let mut intervals = Vec::new();
    for (node, at) in &crashes {
        let mut best = None;
        for (i, (rn, rt)) in recovers.iter().enumerate() {
            if used[i] || *rn != *node || *rt <= *at {
                continue;
            }
            if best.is_none_or(|b: usize| recovers[b].1 > *rt) {
                best = Some(i);
            }
        }
        let Some(i) = best else {
            return false;
        };
        used[i] = true;
        intervals.push((*node, *at, recovers[i].1));
    }
    if used.iter().any(|u| !*u) {
        return false;
    }
    for i in 0..intervals.len() {
        for j in (i + 1)..intervals.len() {
            if intervals[i].0 != intervals[j].0 {
                continue;
            }
            let (a0, a1) = (intervals[i].1, intervals[i].2);
            let (b0, b1) = (intervals[j].1, intervals[j].2);
            if a0 < b1 && b0 < a1 {
                return false;
            }
        }
    }
    true
}

fn is_down(downs: &[(NodeId, u64, u64)], id: NodeId, at: Timestamp) -> bool {
    downs
        .iter()
        .any(|(n, from, until)| *n == id && at.0 >= *from && at.0 < *until)
}

fn window_busy(downs: &[(NodeId, u64, u64)], id: NodeId, from: u64, until: u64) -> bool {
    downs
        .iter()
        .any(|(n, a, b)| *n == id && from < *b && *a < until)
}

fn pick_live(rng: &mut Rng, n: u8, downs: &[(NodeId, u64, u64)], at: Timestamp) -> Option<NodeId> {
    let live: Vec<NodeId> = (0..n)
        .map(NodeId)
        .filter(|id| !is_down(downs, *id, at))
        .collect();
    if live.is_empty() {
        return None;
    }
    Some(live[pick_idx(rng, live.len())])
}

fn pick_free_window(
    rng: &mut Rng,
    n: u8,
    downs: &[(NodeId, u64, u64)],
    from: u64,
    until: u64,
) -> Option<NodeId> {
    let free: Vec<NodeId> = (0..n)
        .map(NodeId)
        .filter(|id| !window_busy(downs, *id, from, until))
        .collect();
    if free.is_empty() {
        return None;
    }
    Some(free[pick_idx(rng, free.len())])
}

fn encode_extras(extras: &[(Timestamp, WorldEvent)]) -> Vec<u8> {
    let mut body = Vec::new();
    let mut n = 0u32;
    for (at, ev) in extras {
        let mut one = Vec::new();
        put_u64_le(&mut one, at.0);
        if encode_extra(&mut one, ev) {
            body.extend_from_slice(&one);
            n = n.saturating_add(1);
        }
    }
    let mut buf = Vec::new();
    put_u32_le(&mut buf, n);
    buf.extend_from_slice(&body);
    buf
}

fn encode_extra(buf: &mut Vec<u8>, ev: &WorldEvent) -> bool {
    match ev {
        WorldEvent::Crash { node, torn_extra } => {
            buf.push(0);
            buf.push(node.0);
            put_u64_le(buf, torn_extra.unwrap_or(u64::MAX));
            true
        }
        WorldEvent::Partition {
            from,
            to,
            connected,
            asymmetric,
        } => {
            buf.push(1);
            buf.push(from.0);
            buf.push(to.0);
            buf.push(u8::from(*connected));
            buf.push(u8::from(*asymmetric));
            true
        }
        WorldEvent::ClientInject { node, req } => {
            buf.push(2);
            buf.push(node.0);
            put_u64_le(buf, req.client.0);
            put_u64_le(buf, req.request.0);
            match &req.cmd {
                Cmd::Get { key } => {
                    buf.push(0);
                    put_u32_le(buf, key.len() as u32);
                    buf.extend_from_slice(key);
                }
                Cmd::Put { key, value } => {
                    buf.push(1);
                    put_u32_le(buf, key.len() as u32);
                    buf.extend_from_slice(key);
                    put_u32_le(buf, value.len() as u32);
                    buf.extend_from_slice(value);
                }
            }
            true
        }
        WorldEvent::Recover { node } => {
            buf.push(3);
            buf.push(node.0);
            true
        }
        WorldEvent::FailNextFsync { node } => {
            buf.push(4);
            buf.push(node.0);
            true
        }
        _ => false,
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len().saturating_mul(2));
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    fn is_planned_kind(ev: &WorldEvent) -> bool {
        matches!(
            ev,
            WorldEvent::Crash { .. }
                | WorldEvent::Partition { .. }
                | WorldEvent::ClientInject { .. }
                | WorldEvent::Recover { .. }
                | WorldEvent::FailNextFsync { .. }
        )
    }

    fn client_op_count(extras: &[(Timestamp, WorldEvent)]) -> usize {
        extras
            .iter()
            .filter(|(_, e)| matches!(e, WorldEvent::ClientInject { .. }))
            .count()
    }

    #[test]
    fn seed_7_plan_is_deterministic() {
        let a = swarm_plan(7);
        let b = swarm_plan(7);
        assert_eq!(encode_config(&a.cfg), encode_config(&b.cfg));
        assert_eq!(a.profile, b.profile);
        assert_eq!(encode_extras(&a.extras), encode_extras(&b.extras));
        assert_eq!(a.extras, b.extras);
    }

    #[test]
    fn swarm_never_enables_lie_or_liveness() {
        for seed in 0..64 {
            let plan = swarm_plan(seed);
            assert!(
                !plan.cfg.fsync_ok_but_not_durable,
                "seed {seed} enabled fsync-lie"
            );
            assert!(!plan.cfg.check_liveness, "seed {seed} enabled liveness");
            assert!(plan.cfg.check_safety);
            assert!(plan.cfg.check_engineering);
            assert!(plan.cfg.n == 3 || plan.cfg.n == 5);
            assert!(plan.cfg.torn_suffix);
            assert!(plan.cfg.drop_ppm <= 50_000);
            assert!(client_op_count(&plan.extras) <= MAX_CLIENT_OPS);
            assert!(plan.extras.len() <= MAX_EXTRAS);
            assert!(plan.extras.iter().all(|(_, e)| is_planned_kind(e)));
            assert!(plan.extras.iter().all(|(t, _)| t.0 >= 1));
            let cuts = plan
                .extras
                .iter()
                .filter(|(_, e)| {
                    matches!(
                        e,
                        WorldEvent::Partition {
                            connected: false,
                            ..
                        }
                    )
                })
                .count();
            let heals = plan
                .extras
                .iter()
                .filter(|(_, e)| {
                    matches!(
                        e,
                        WorldEvent::Partition {
                            connected: true,
                            ..
                        }
                    )
                })
                .count();
            assert_eq!(cuts, heals, "seed {seed} partition without heal");
            assert!(
                timestamps_within(&plan.extras, plan.cfg.max_ns),
                "seed {seed} extra past max_ns"
            );
            assert!(
                crash_windows_disjoint(&plan.extras),
                "seed {seed} overlapping crash windows"
            );
        }
    }

    #[test]
    fn heal_past_horizon_is_rejected_by_the_invariant() {
        let extras = vec![
            (
                Timestamp(4_000_000_000),
                WorldEvent::Partition {
                    from: NodeId(0),
                    to: NodeId(1),
                    connected: false,
                    asymmetric: false,
                },
            ),
            (
                Timestamp(7_000_000_000),
                WorldEvent::Partition {
                    from: NodeId(0),
                    to: NodeId(1),
                    connected: true,
                    asymmetric: false,
                },
            ),
        ];
        assert!(!timestamps_within(&extras, 5_000_000_000));
    }

    #[test]
    fn overlapping_crash_windows_are_rejected_by_the_invariant() {
        let extras = vec![
            (
                Timestamp(50),
                WorldEvent::Crash {
                    node: NodeId(0),
                    torn_extra: None,
                },
            ),
            (Timestamp(400), WorldEvent::Recover { node: NodeId(0) }),
            (
                Timestamp(100),
                WorldEvent::Crash {
                    node: NodeId(0),
                    torn_extra: None,
                },
            ),
            (Timestamp(350), WorldEvent::Recover { node: NodeId(0) }),
        ];
        assert!(!crash_windows_disjoint(&extras));
    }

    #[test]
    fn extras_do_not_include_bootstrap_recover_at_t0() {
        let plan = swarm_plan(7);
        assert!(plan.extras.iter().all(|(t, _)| t.0 >= 1));
    }

    #[test]
    fn run_seed_7_is_deterministic() {
        let a = run_seed(7);
        let b = run_seed(7);
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.profile, b.profile);
        assert_eq!(encode_config(&a.cfg), encode_config(&b.cfg));
    }

    #[test]
    fn run_plan_matches_run_seed() {
        let plan = swarm_plan(7);
        let via_plan = run_plan(7, plan.profile, plan.cfg.clone(), plan.extras.clone());
        let via_seed = run_seed(7);
        assert_eq!(via_plan.digest, via_seed.digest);
        assert_eq!(via_plan.check, via_seed.check);
        assert_eq!(via_plan.extras, via_seed.extras);
    }

    #[test]
    fn fail_file_roundtrip_seed() {
        let report = run_seed(7);
        let bytes = format_fail_file(&report);
        assert_eq!(seed_from_fail_file(&bytes), Some(7));
        let pos = bytes.windows(4).position(|w| w == b"\n--\n").unwrap();
        let header = std::str::from_utf8(&bytes[..pos]).unwrap();
        assert!(header.contains("chronos-fail 1"));
        assert!(header.contains(&digest_hex(&report.digest)));
    }

    #[test]
    fn seed_from_fail_file_rejects_garbage() {
        assert_eq!(seed_from_fail_file(b"not a fail file"), None);
        assert_eq!(fail_file_header(b"not a fail file"), None);
        assert_eq!(seed_from_fail_file(b"seed 7\n"), None);
    }

    #[test]
    fn fail_file_header_rejects_unknown_check_label() {
        let digest = "ab".repeat(32);
        let text = format!(
            "chronos-fail 1\nseed 1\ndigest {digest}\ncheck NotACheck\nconfig 00\nextras 00\n--\n"
        );
        assert!(fail_file_header(text.as_bytes()).is_none());
    }

    fn stub_report(digest: [u8; 32], check: Option<CheckFail>) -> RunReport {
        RunReport {
            seed: 7,
            profile: Profile::Calm,
            cfg: SimConfig::default(),
            extras: Vec::new(),
            digest,
            check,
            encoded_trace: b"trace".to_vec(),
            counters: Coverage {
                n: 3,
                brutal: false,
                crash: false,
                partition: false,
                fsync_err: false,
                drop: false,
                dup: false,
                buggify: false,
                torn: false,
            },
        }
    }

    #[test]
    fn fail_file_header_parses_seed_digest_and_check() {
        let digest = [0xab; 32];
        let report = stub_report(
            digest,
            Some(CheckFail::new(CheckName::ElectionSafety, "two leaders")),
        );
        let bytes = format_fail_file(&report);
        let header = fail_file_header(&bytes).expect("header");
        assert_eq!(header.seed, 7);
        assert_eq!(header.digest, digest);
        assert_eq!(header.check, Some(CheckName::ElectionSafety));
        assert_eq!(seed_from_fail_file(&bytes), Some(7));
    }

    #[test]
    fn fail_file_header_accepts_crlf() {
        let report = stub_report(
            [0xcd; 32],
            Some(CheckFail::new(CheckName::LogMatching, "x")),
        );
        let lf = format_fail_file(&report);
        let crlf: Vec<u8> = String::from_utf8(lf)
            .unwrap()
            .replace('\n', "\r\n")
            .into_bytes();
        let header = fail_file_header(&crlf).expect("crlf header");
        assert_eq!(header.check, Some(CheckName::LogMatching));
        assert_eq!(header.digest, [0xcd; 32]);
    }

    #[test]
    fn fail_file_header_survives_non_utf8_trace() {
        let mut report = stub_report(
            [0x11; 32],
            Some(CheckFail::new(CheckName::LogMatching, "x")),
        );
        report.encoded_trace = vec![0xff, 0xfe, b'\n', b'-', b'-', b'\n', 0x80];
        let header = fail_file_header(&format_fail_file(&report)).expect("binary trace");
        assert_eq!(header.seed, 7);
        assert_eq!(header.check, Some(CheckName::LogMatching));
        assert_eq!(header.digest, [0x11; 32]);
    }

    #[test]
    fn verify_replay_reproduces_matching_fail() {
        let digest = [1u8; 32];
        let check = Some(CheckFail::new(CheckName::LogMatching, "prefix"));
        let report = stub_report(digest, check.clone());
        let header = fail_file_header(&format_fail_file(&report)).unwrap();
        assert_eq!(verify_replay(&header, &report), ReplayVerdict::Reproduced);
        assert_eq!(ReplayVerdict::Reproduced.exit_code(), 1);
        assert_eq!(ReplayVerdict::Clean.exit_code(), 0);
        assert_eq!(ReplayVerdict::DidNotReproduce.exit_code(), 0);
        assert_eq!(ReplayVerdict::DigestMismatch.exit_code(), 2);
        assert_eq!(ReplayVerdict::CheckMismatch.exit_code(), 2);
        assert_eq!(ReplayVerdict::ConfigMismatch.exit_code(), 2);
        assert_eq!(ReplayVerdict::ExtrasMismatch.exit_code(), 2);
    }

    #[test]
    fn verify_replay_header_fail_and_live_clean_did_not_reproduce() {
        let original = stub_report(
            [1u8; 32],
            Some(CheckFail::new(CheckName::ElectionSafety, "was")),
        );
        let header = fail_file_header(&format_fail_file(&original)).unwrap();
        let fixed = stub_report([2u8; 32], None);
        assert_eq!(
            verify_replay(&header, &fixed),
            ReplayVerdict::DidNotReproduce
        );
    }

    #[test]
    fn verify_replay_both_clean_is_clean() {
        let original = stub_report([1u8; 32], None);
        let header = fail_file_header(&format_fail_file(&original)).unwrap();
        let again = stub_report([1u8; 32], None);
        assert_eq!(verify_replay(&header, &again), ReplayVerdict::Clean);
    }

    #[test]
    fn verify_replay_digest_mismatch_while_still_failing() {
        let original = stub_report(
            [1u8; 32],
            Some(CheckFail::new(CheckName::ElectionSafety, "a")),
        );
        let header = fail_file_header(&format_fail_file(&original)).unwrap();
        let dirty = stub_report(
            [2u8; 32],
            Some(CheckFail::new(CheckName::ElectionSafety, "a")),
        );
        assert_eq!(
            verify_replay(&header, &dirty),
            ReplayVerdict::DigestMismatch
        );
    }

    #[test]
    fn verify_replay_check_mismatch_same_digest() {
        let original = stub_report(
            [1u8; 32],
            Some(CheckFail::new(CheckName::ElectionSafety, "a")),
        );
        let header = fail_file_header(&format_fail_file(&original)).unwrap();
        let other = stub_report([1u8; 32], Some(CheckFail::new(CheckName::LogMatching, "b")));
        assert_eq!(verify_replay(&header, &other), ReplayVerdict::CheckMismatch);
    }

    #[test]
    fn verify_replay_config_mismatch_same_digest_and_check() {
        let original = stub_report(
            [1u8; 32],
            Some(CheckFail::new(CheckName::ElectionSafety, "a")),
        );
        let header = fail_file_header(&format_fail_file(&original)).unwrap();
        let mut tampered = original;
        tampered.cfg.n = 5;
        assert_eq!(
            verify_replay(&header, &tampered),
            ReplayVerdict::ConfigMismatch
        );
    }

    #[test]
    fn verify_replay_extras_mismatch_same_digest_and_check() {
        let original = stub_report(
            [1u8; 32],
            Some(CheckFail::new(CheckName::ElectionSafety, "a")),
        );
        let header = fail_file_header(&format_fail_file(&original)).unwrap();
        let mut tampered = original;
        tampered
            .extras
            .push((Timestamp(9), WorldEvent::Recover { node: NodeId(0) }));
        assert_eq!(
            verify_replay(&header, &tampered),
            ReplayVerdict::ExtrasMismatch
        );
    }

    #[test]
    fn format_replay_line_mismatch_is_not_fail() {
        let report = stub_report(
            [1u8; 32],
            Some(CheckFail::new(CheckName::ElectionSafety, "a")),
        );
        let header = fail_file_header(&format_fail_file(&report)).unwrap();
        let dirty = stub_report(
            [2u8; 32],
            Some(CheckFail::new(CheckName::ElectionSafety, "a")),
        );
        let line = format_replay_line(ReplayVerdict::DigestMismatch, &header, &dirty);
        assert!(line.starts_with("MISMATCH digest "));
        assert!(!line.starts_with("FAIL"));
        assert!(!line.starts_with("ABORT"));
    }

    #[test]
    fn run_seed_fail_file_roundtrip_verify() {
        let a = run_seed(7);
        let header = fail_file_header(&format_fail_file(&a)).unwrap();
        let b = run_seed(7);
        let verdict = verify_replay(&header, &b);
        assert_eq!(a.digest, b.digest);
        match (&a.check, verdict) {
            (None, ReplayVerdict::Clean) => {}
            (Some(_), ReplayVerdict::Reproduced) => {}
            other => panic!("unexpected roundtrip {other:?}"),
        }
        let line = format_replay_line(verdict, &header, &b);
        assert!(!line.starts_with("FAIL"));
        assert!(!line.starts_with("ok "));
    }

    #[test]
    fn format_planned_schedule_is_line_oriented() {
        let extras = vec![
            (
                Timestamp(50),
                WorldEvent::Crash {
                    node: NodeId(0),
                    torn_extra: None,
                },
            ),
            (Timestamp(100), WorldEvent::Recover { node: NodeId(0) }),
        ];
        let text = format_planned_schedule(&extras);
        assert!(text.contains("t=50 "));
        assert!(text.contains("t=100 "));
        assert!(text.contains("Crash"));
        assert!(text.contains("Recover"));
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn adjacent_crash_windows_are_disjoint() {
        let extras = vec![
            (
                Timestamp(50),
                WorldEvent::Crash {
                    node: NodeId(0),
                    torn_extra: None,
                },
            ),
            (Timestamp(100), WorldEvent::Recover { node: NodeId(0) }),
            (
                Timestamp(100),
                WorldEvent::Crash {
                    node: NodeId(0),
                    torn_extra: None,
                },
            ),
            (Timestamp(150), WorldEvent::Recover { node: NodeId(0) }),
        ];
        assert!(crash_windows_disjoint(&extras));
    }

    #[test]
    fn encode_extras_skips_unplanned_kinds() {
        let extras = vec![(
            Timestamp(1),
            WorldEvent::Dropped {
                from: NodeId(0),
                to: NodeId(1),
                msg_id: chronos_protocol::MsgId(0),
                reason: crate::scheduler::DropReason::Loss,
            },
        )];
        let buf = encode_extras(&extras);
        assert_eq!(&buf[..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn run_seed_7_is_clean() {
        let report = run_seed(7);
        assert!(report.check.is_none(), "seed 7 failed a checker");
        assert!(timestamps_within(&report.extras, report.cfg.max_ns));
        assert!(crash_windows_disjoint(&report.extras));
    }

    #[test]
    fn coverage_is_observed_not_configured() {
        for seed in 0..64 {
            let plan = swarm_plan(seed);
            let has_crash = plan
                .extras
                .iter()
                .any(|(_, e)| matches!(e, WorldEvent::Crash { .. }));
            if plan.cfg.dup_ppm == 0 {
                let report = run_seed(seed);
                assert!(!report.counters.dup, "seed {seed} dup without dup_ppm");
            }
            if !has_crash {
                let report = run_seed(seed);
                assert!(
                    !report.counters.torn,
                    "seed {seed} torn without a crash window"
                );
            }
        }
    }

    #[test]
    fn aggregate_coverage_counts_singles_and_pairs() {
        let a = Coverage {
            n: 3,
            brutal: false,
            crash: true,
            partition: true,
            fsync_err: false,
            drop: false,
            dup: false,
            buggify: false,
            torn: false,
        };
        let b = Coverage {
            n: 5,
            brutal: true,
            crash: true,
            partition: false,
            fsync_err: false,
            drop: false,
            dup: false,
            buggify: false,
            torn: false,
        };
        let summary = aggregate_coverage([&a, &b]);
        assert_eq!(summary.runs, 2);
        assert_eq!(summary.observed.get("crash"), Some(&2));
        assert_eq!(summary.observed.get("partition"), Some(&1));
        assert_eq!(summary.profile.get("n3"), Some(&1));
        assert_eq!(summary.profile.get("n5"), Some(&1));
        assert_eq!(summary.profile.get("brutal"), Some(&1));
        assert!(!summary.observed.contains_key("brutal"));
        assert_eq!(summary.pairs.get(&("crash", "partition")), Some(&1));
        assert!(!summary.pairs.contains_key(&("brutal", "crash")));
        assert!(!summary.pairs.contains_key(&("crash", "n3")));
    }

    #[test]
    fn coverage_flags_reports_n_other_when_not_3_or_5() {
        let c = Coverage {
            n: 7,
            brutal: false,
            crash: false,
            partition: false,
            fsync_err: false,
            drop: false,
            dup: false,
            buggify: false,
            torn: false,
        };
        assert_eq!(coverage_profile_flags(&c), vec!["n_other"]);
        assert!(coverage_observed_flags(&c).is_empty());
        let summary = aggregate_coverage([&c]);
        assert_eq!(summary.profile.get("n_other"), Some(&1));
        assert!(!summary.profile.contains_key("n3"));
        assert!(!summary.profile.contains_key("n5"));
        assert!(summary.observed.is_empty());
    }

    #[test]
    fn format_coverage_separates_profile_knobs_from_observed_faults() {
        let c = Coverage {
            n: 3,
            brutal: true,
            crash: false,
            partition: false,
            fsync_err: false,
            drop: false,
            dup: false,
            buggify: true,
            torn: false,
        };
        let text = format_coverage_table(&aggregate_coverage([&c]));
        assert!(text.contains("# profile"));
        assert!(text.contains("# observed"));
        assert!(text.contains("profile=run knobs"));
        assert!(text.contains("observed=faults that fired"));
        let profile_at = text.find("# profile\n").expect("profile section");
        let observed_at = text.find("# observed\n").expect("observed section");
        assert!(profile_at < observed_at);
        let profile = &text[profile_at..observed_at];
        let observed = &text[observed_at..];
        assert!(profile.contains("brutal 1"));
        assert!(profile.contains("buggify 1"));
        assert!(profile.contains("n3 1"));
        assert!(!observed.contains("brutal"));
        assert!(!observed.contains("buggify"));
        assert!(observed.contains("(none)") || !observed.contains("brutal 1"));
    }

    #[test]
    fn format_coverage_table_states_batch_claim() {
        let a = Coverage {
            n: 3,
            brutal: false,
            crash: true,
            partition: true,
            fsync_err: false,
            drop: false,
            dup: false,
            buggify: false,
            torn: false,
        };
        let text = format_coverage_table(&aggregate_coverage([&a]));
        assert!(text.contains("# coverage runs=1"));
        assert!(text.contains("not all combinations"));
        assert!(text.contains("crash 1"));
        assert!(text.contains("crash+partition 1"));
        assert!(text.contains("observed same-run co-occurrence"));
    }
}
