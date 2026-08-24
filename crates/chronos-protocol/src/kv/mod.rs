//! KV state machine. Apply happens inside `step`, not as an Effect.
//!
//! Fill in: Phase 1 (single-node), Phase 3 (behind the log).
//! Spec: `docs/02-architecture.md` D7, D8, D12.

pub mod machine;
