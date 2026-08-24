//! Raft role, log, election, and replication. The only transition is `step`.
//!
//! Spec: `docs/02-architecture.md` § Raft vs KV.

pub mod election;
pub mod log;
pub mod replication;
pub mod state;
pub mod step;
