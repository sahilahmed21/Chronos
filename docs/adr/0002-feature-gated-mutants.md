# ADR-0002: Gate protocol mutants behind a Cargo feature

**Date**: 2026-11-10
**Status**: accepted
**Deciders**: Sahil

## Context

`skip_vote_persist` is a runtime field and public setter in the production crate (`crates/chronos-protocol/src/raft/state.rs`), although the README described it as `#[cfg(test)]`-only. Phase 1 adds more bug hooks to measure checker detection power. `cfg(test)` is invisible to the `chronos-sim` binary and to integration tests, which are separate crates.

## Decision

Add Cargo feature `mutants` to `chronos-protocol` (forwarded by `chronos-sim`). It holds a single `Option<Mutant>` on `Node`. In default builds the hook branches are dead code (`Node::is_mutant` is const-false) and `Node::set_mutant(Some(_))` panics, so a production binary cannot run with a bug selected. The branches are still type-checked in both configurations; this was chosen over `#[cfg]` on every site to keep the diff small.

## Alternatives Considered

### Alternative 1: one bool per hook (status quo)
- **Pros**: matches existing code.
- **Cons**: does not scale to 6 to 8 hooks; ships bug hooks in the production binary.
- **Why not**: the production build must not contain deliberate bugs.

### Alternative 2: `#[cfg(test)]`
- **Pros**: matches the README's original claim.
- **Cons**: not visible to the sim CLI or integration tests.
- **Why not**: cannot drive the swarm.

## Consequences

### Positive
- Docs become true; one mechanism for all mutants.

### Negative
- Two CI build configurations.

### Risks
- A leaky `cfg` site could change default behavior. Mitigation: with the feature on and no mutant selected, seed digests must equal the default build's.
