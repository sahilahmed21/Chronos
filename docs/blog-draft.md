# DRAFT: Your agent's retry is a distributed-systems bug

Status: draft for the author to rewrite in their own voice. Every number below was measured with scripted agents, not a real model. Record/replay and real-model evals live in Atlas, a separate repo.

## The failure

A refund agent calls `charge`. The call times out, but the payment provider had already committed it (a timeout after commit). The agent retries without an idempotency key, and the customer is charged twice. Nothing in the agent's output looks wrong; it reports success.

That is the same bug class as a Raft client retrying a `Put` without a request id. In Chronos the idempotency table lives in the state machine (`crates/chronos-protocol/src/kv/machine.rs`), and removing it is one of the six mutants the swarm has to catch.

## Make it reproducible

`chronos_agent` runs the agent against a simulated tool world where every fault is a seeded, replayable event:

```text
$ python -m chronos_agent demo
seed 9 (brutal): 6 faults injected, 7 tool calls
violation: duplicate_effect - charge/refund applied 2x
shrunk 6 -> 1 fault(s): [(1, 'timeout_after_commit')]
replay digest e87e96d181c6bdd7 == e87e96d181c6bdd7: True
good agent: no violations
```

Delta debugging cuts six injected faults to the one that matters, and the tool emits a pytest regression that fails on the buggy agent and passes on the fixed one.

## Does the checker catch real bugs?

Five planted agent bugs, 2,000 brutal fault schedules: all five detected, and the reference agent produced zero false alarms. Over seeds 1..399 the median shrink went from five faults to one. The same discipline on the Raft side: six deliberate protocol bugs, four caught within 2,000 seeds (two of those only 4 and 1 times), all six within 100,000 (README, "Oracle detection power"). A checker you have not attacked is a guess.

## Reliability is a distribution

Over 1,000 fault schedules per profile, the reference agent succeeds 100% / 93% / 75% (no faults / mild / brutal). Asking it to succeed on five independent draws in a row (pass^5) gives 100% / 70% / 24%. The retry-without-key agent drops to 52% and 3.6% in the brutal profile. Anthropic's evals post explains why consistency, not best-of-k, is the number that matters for agents you ship ([Demystifying evals for AI agents](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents), 2026-01-09).

## What this does not show

- The agents above are scripted functions, so pass^k here measures robustness to faults, not model sampling noise.
- No real model is involved here. For record/replay of model calls and evals against a real model, see Atlas (separate repo); Chronos does not duplicate that.

## Prior art

Record-and-replay for agents is young: [Chronicle](https://arxiv.org/abs/2609.20625) (cut-point replay, 6 incidents, simulated model boundaries) and a small seeded simulator at [rouviere-german/deterministic-agent-simulator](https://github.com/rouviere-german/deterministic-agent-simulator). The method comes from FoundationDB-style simulation testing.
