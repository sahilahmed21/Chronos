//! Deterministic simulation harness. Interprets protocol `Effect`s.
//!
//! Spec: `docs/02-architecture.md`. No dependency on `chronos-node`.

pub mod check;
pub mod cluster;
pub mod disk;
pub mod fuzz;
pub mod history;
pub mod minify;
pub mod net;
pub mod rng;
pub mod scheduler;
pub mod trace;

pub use check::{CheckFail, CheckName, Checker, NodeSnap};
pub use cluster::{
    run, Cluster, DelayBind, DeliveryToken, ObservedSchedule, ReplayBook, SendKey, SimConfig,
};
pub use fuzz::{
    aggregate_coverage, coverage_flags, coverage_observed_flags, coverage_profile_flags,
    encode_config, fail_file_header, format_coverage_table, format_fail_file,
    format_planned_schedule, format_replay_line, run_plan, run_seed, seed_from_fail_file,
    swarm_plan, verify_replay, Coverage, CoverageSummary, FailFileHeader, FaultConfig, Profile,
    ReplayVerdict, RunReport, SwarmPlan,
};
pub use history::History;
pub use minify::{
    format_min_schedule, minify, minify_input, Atom, MinResult, MinifyInput, MinifyOutcome,
};
pub use net::{Delivery, DropRule, RpcKind, SendOutcome};
pub use rng::Rng;
pub use scheduler::{DropReason, Scheduler, WorldEvent};
pub use trace::digest_hex;
