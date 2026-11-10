# ADR-0001: Build the agent reliability harness in Python

**Date**: 2026-11-10
**Status**: accepted, partly superseded 2026-11-10. The recorder, `llm_agent` and OTel export described below were removed from Chronos; record/replay and real-model evals now live in Atlas (a separate repo). The language decision (Python) and the fault-injection and shrinking parts stand.
**Deciders**: Sahil

## Context

Phase 2 of `NEXT_PHASES.md` needs record/replay, fault injection and trajectory shrinking for tool-using LLM agents. Chronos' reusable Rust is about 70 lines of ddmin plus a SHA-256 digest. HTTP-level LLM recorders already exist in Python (see `INVESTIGATION.md` section 7). The time budget is about 100 hours.

## Decision

The harness lives in `agent/` as a Python package (`chronos_agent`): an httpx transport recorder, a seeded simulated tool world, per-call invariants and a ddmin shrinker. Chronos is referenced as method, not imported as code. The Rust workspace and its determinism gates are untouched.

## Alternatives Considered

### Alternative 1: Rust HTTP proxy plus Python demo agent
- **Pros**: more Rust; framework-agnostic over HTTP.
- **Cons**: needs an HTTP stack in a dependency-free workspace; reimplements existing recorders; SSE handling is the highest-risk piece.
- **Why not**: almost nothing would be shared with Chronos.

### Alternative 2: use an existing recorder unmodified
- **Pros**: zero build.
- **Cons**: no fault injection, invariants or shrinking.
- **Why not**: those are the point of the project.

## Consequences

### Positive
- Same language as the agents the author builds at work.
- About 8 hours less than the Rust proxy path.

### Negative
- Weaker Rust signal.
- Tied to httpx-based SDKs.

### Risks
- Streaming responses: v1 raises `Unsupported` rather than recording partial data.
- A real-model run has not been executed (no API key or budget); the recorder is tested against `httpx.MockTransport` only.
