# INVESTIGATION.md — three open Chronos decisions

Date: 2026-11-03. Repo: `C:\projects\Chronos` @ `977299c` plus uncommitted doc-comment changes. Follow-up to `NEXT_PHASES.md`.

## 1. Executive summary

The investigation template had no fields filled in. You chose all three candidate topics, so this report resolves three decisions:

| # | Decision | Verdict |
|---|---|---|
| A | How to build the Phase 2 agent harness (Rust proxy + Python demo vs all-Python) | **All-Python, record at the httpx layer. This reverses the recommendation in `NEXT_PHASES.md`.** |
| B | Does the linearizability checker (`history.rs`) need scaling work? | **No. Leave it alone until the workload grows.** The swarm is not checker-bound. |
| C | How to plant the Phase 1 mutants | **A Cargo feature `mutants` on `chronos-protocol`, one `Mutant` enum field.** Detection target: seeds-to-detect measured up to 100k, not "within 1,000". |

Two findings change claims made earlier:

1. **`skip_vote_persist` is not test-only.** README (lines 44, 102) and `docs/HANDOFF.md` say it is `#[cfg(test)]` only. It is a plain runtime field and public setter in the production protocol crate (`crates/chronos-protocol/src/raft/state.rs:101`, `:140-143`), branched on in `election.rs:39` and `:144`. Nothing in the type system or build prevents `chronos-node` from enabling it. The true guarantee is "swarm and CLI minify never call it" (`fuzz.rs:368` only passes what the planted test supplies).
2. **The 1000-seed swarm costs about 2 ms per seed.** In the nightly run `41463316257`, the swarm step started at 14:35:37.5 and printed `# coverage runs=1000` at 14:35:39.7 (`gh run view 41463316257 --log`, lines 661 and 1672). So 100k seeds is roughly 3 to 4 minutes on that runner. The cheap lever is seed count and horizon, not checker algorithms. This is an inference from two log timestamps; I did not run it myself (see limitation below).

**Biggest limitation.** I could not execute any experiment. Native `cargo build` of `chronos-sim` fails at link time because the `link.exe` on PATH is Git's coreutils `link` (error: `link: extra operand ...rcgu.o`), not MSVC's linker. `chronos-protocol` alone compiles. Docker Desktop's daemon is stopped, and the WSL Ubuntu distro returned `E_UNEXPECTED`. I did not start or repair either (that would change your machine). Everything below marked "not executed" has a reproducible plan.

## 1b. Verification pass (2026-11-04) — what was confirmed, corrected, still open

Each claim was re-checked against a primary source or the code. Corrections override the text below where they conflict.

**Corrected (my earlier text was wrong):**
- **The Anthropic and OpenAI Python SDKs do not use `httpx`; they use `httpx2`.** Anthropic SDK `2.4.0` `pyproject.toml` lists `httpx2 (>=2.0.0,<3)`; `_client.py` takes `http_client: httpx2.Client | None` and `base_url`, with precedence "kwarg > ANTHROPIC_BASE_URL > profile config". OpenAI's `pyproject.toml` lists `httpx2>=2.12.0,<3`. `httpx2` is Pydantic's continuation of httpx (PyPI page; import name `httpx2`; BSD-3, 1,621 stars, pushed 2026-11-03).
- **The existing recorders target `httpx`, not `httpx2`.** PyPI metadata: `pytest-llm-vcr 0.6.0` requires `httpx>=0.27`; `promptecho 0.1.3` (alpha) requires `httpx>=0.24` and its page never mentions `httpx2`. Whether either intercepts a current SDK client is **not tested**; by their declared dependency it likely does not. This strengthens decision A2 (own transport) but means "an existing recorder can be adopted" is unconfirmed. Neither tool offers fault injection (per their pages).
- **A2 is technically feasible.** `httpx2` source: `_transports/base.py` defines `BaseTransport.handle_request` (line 27) and `AsyncBaseTransport.handle_async_request` (line 76); `_transports/mock.py` defines `MockTransport`; `_client.py` takes `transport: BaseTransport | None`. So a recording transport can be passed through `http_client=httpx2.Client(transport=...)`. Not executed.
- **Figure 8 detectability is less pessimistic than I wrote.** Swarm runs with `check_engineering: true` (`fuzz.rs:335`), so `CurrentTermCommit` is live. The check (`check.rs:403-420`) fires whenever a leader's commit index advances onto an entry whose term differs from `currentTerm`, so the mutant needs only a new leader to see an old-term entry as the highest majority-replicated index before its own no-op is majority-replicated, not a full overwrite. I still have not run it; treat detection rate as unknown.
- **Swarm timing is stable across runs:** four nightly runs (`gh run view --log`) show 1000 seeds in 2.16 s, 2.20 s, 2.21 s, 1.72 s (about 2 ms/seed), 1000 `ok seed=` lines each.
- **Local build host:** no Visual Studio, no Windows Kits directory, no `vswhere.exe` exist on this machine, so MSVC is simply not installed (not just shadowed on PATH). A hypervisor is present, but Docker's daemon is stopped and WSL's Ubuntu fails with `E_UNEXPECTED`. Fixing needs a machine change (install Build Tools, or repair Docker/WSL), which I did not make without your say-so.

**Confirmed:**
- `skip_vote_persist` is a runtime field, not `cfg(test)`: `election.rs:39,144` and `step.rs:62-66` carry it across `recover`; README lines 44 and 102 and `docs/HANDOFF.md:58` claim test-only.
- Swarm workload: two keys, ≤40 ops (`fuzz.rs:17,709-713,729-733`).
- [Reliability without Validity](https://arxiv.org/abs/2606.19544) (submitted 2026-06-17): 21 judges, 9 providers, about 541,000 judgments; reliability above 0.95 with position bias above 0.10 in two judges; exact match overstates vs kappa by 33 to 41 points on MT-Bench.
- [τ-bench](https://arxiv.org/abs/2406.12045) (2024-06-17): pass^k metric; gpt-4o under 50% of tasks; pass^8 under 25% in retail.
- [Horn and Kroening](https://arxiv.org/abs/1504.00204), FORTE 2015: P-compositionality, about an order of magnitude faster than WGL on nine concurrent sets.
- [Anthropic sandbox-runtime](https://github.com/anthropic-experimental/sandbox-runtime) (`anthropic-experimental/sandbox-runtime`, Apache-2.0, 6,946 stars, pushed 2026-10-30): `sandbox-exec` on macOS, `bubblewrap` on Linux, **and native Windows support** (dedicated `srt-sandbox` user, Windows Filtering Platform egress fence, write-restricted token; the README lists known limitations). Labeled "Beta Research Preview". So Phase 3 does not require WSL or Docker on Windows; whether it works on your Windows edition is untested.
- Funding: Antithesis $105M Series A led by Jane Street, 2025-12-03 ([PR Newswire](https://tools.prnewswire.com/en-us/live/20823/release/20251203EN37581)); Braintrust $80M Series B led by Iconiq at $800M, 2026-02-17 ([SiliconANGLE](https://siliconangle.com/2026/02/17/braintrust-lands-80m-series-b-funding-round-become-observability-layer-ai)); Daytona $60M Series B at $1B led by FirstMark, 2026-10-20 ([Dealroom](https://dealroom.co/news/164304-daytona-raises-60m-series-b-at-1b-valuation-to-build-the-agentic-cloud/)); ClickHouse acquired Langfuse, 2026-01-16, Langfuse stays MIT ([ClickHouse](https://clickhouse.com/blog/clickhouse-acquires-langfuse-open-source-llm-observability)).
- Temporal + OpenAI Agents SDK integration reached GA on 2026-03-23 ([Temporal](https://temporal.io/blog/announcing-openai-agents-sdk-integration)). That page makes **no** Codex claim.
- OpenTelemetry GenAI: all `gen_ai.*` attributes at Development; conventions moved to `semantic-conventions-genai` on 2026-06-12 in v1.42.0; no tagged release in the new repo ([Dash0](https://www.dash0.com/knowledge/opentelemetry-genai-semantic-conventions-explained)).

**Still unverified (no primary source reached or no way to execute):**
- Every runtime number: checker scaling curve, mutant detection rates, Phase 2 overheads. Needs a working build host.
- Town job listing (403 on fetch); LangChain and Glean listings and E2B/Modal funding (not re-fetched).
- Codex-runs-on-Temporal claim (no OpenAI or Temporal page I fetched states it).
- MAST's LLM-judge κ 0.77: the arXiv abstract gives only κ 0.88 for inter-annotator agreement.
- Whether `pytest-llm-vcr` or `promptecho` work with `httpx2` clients.
- Whether SSE streaming works through a custom `httpx2` transport with the Anthropic SDK.

## 2. Problem definition and current state

- **Existing system (verified by reading):** Raft + KV `step` function (`chronos-protocol`), deterministic simulator and checkers (`chronos-sim`), WAL demo (`chronos-node`). Green nightly CI through 2026-11-02.
- **Constraints:** about 100 usable hours until end of November; Windows dev box without MSVC; no ability to build locally right now; `chronos-protocol` and `chronos-sim` are dependency-free by design (D5, D6, D16).
- **Hard-to-reverse choices:** the language and repo layout of the agent harness (A); the mutant mechanism, because it touches `Node` state and every call site (C). The checker (B) is cheap to change later.

## 3. Fundamentals

- **Linearizability checking** (Wing-Gong): search over orders of operations consistent with real-time precedence; memoize failed `(remaining-set, state)` pairs. Worst-case cost depends on how many operations are mutually concurrent, not total count. Per-key decomposition is valid for independent keys (P-compositionality, Horn and Kroening 2015), because a history over independent objects is linearizable iff each per-object sub-history is.
- **Mutation testing as oracle validation:** plant a known bug; the oracle is credited only if it fails, and any false alarm on clean code counts against it. This is the same idea as Anthropic's advice to validate graders by reading failures ([Demystifying evals](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents), 2026-01-09, fetched).
- **LLM record/replay:** record each non-deterministic boundary, replay by serving recordings, fail on an unrecorded call ([Tian Pan, 2026-04-12](https://tianpan.co/blog/2026-04-12-deterministic-replay-debugging-non-deterministic-ai-agents), fetched).

## 4 and 5. Data flow and code findings (source-code observations)

**Checker (`crates/chronos-sim/src/history.rs`).**
- `MAX_OPS = 48` (line 13). Operations are indexed as bits in a `u64` (`preds`, `remaining`), so the hard ceiling is 64; 48 is a policy cap, not an algorithmic limit.
- `linearizable()` (lines 83-118) builds `preds[j]` from completed ops whose completion precedes `j`'s invoke. Incomplete ops are never predecessors, which is correct.
- `search()` (lines 154-213) memoizes failures in `LinCache = BTreeSet<(u64, BTreeMap<..>)>` and clones the whole map per branch. Memory is the first thing to blow up.
- Outcomes: definite fail (`Invalid`, `NotLeader`) skips the op; `Io` and no-reply try both skip and apply; other replies must equal the spec result. This is the seed-281 fix.
- Swarm workload: at most 40 client ops (`fuzz.rs:17`), 8 to 32 typical (`fuzz.rs:709-713`), **two keys `k` and `m` picked 50/50** (`fuzz.rs:729-733`), 25% Gets, values `a` or `b`. So the "single key" caveat in `NEXT_PHASES.md` Phase 1 is resolved: per-key splitting would roughly halve each sub-history.

**Mutant hook (current pattern).** One `bool` field per hook on `Node` (`state.rs:101`, `reject_ok_ae` likewise), a public setter, and a parallel `Vec<bool>` in `Cluster` (`cluster.rs:247`, `:281`, `:397-402`) so the flag survives recover (`cluster.rs:764`; `step.rs:62-66` carries it across `recover`). Adding N mutants this way means N fields, N setters, N vectors.

**Commit rule (mutant target).** `maybe_commit` (`raft/replication.rs:469-488`) skips entries not from `current_term` at line 476. Deleting that `if` is the Figure 8 mutant. The checker `CurrentTermCommit` exists in `check.rs` to catch exactly that.

**Idempotency (mutant target).** `kv/machine.rs` `apply` returns the stored response when `(client, request)` is in the table; removing that lookup is a one-line mutant.

## 6. Production engineering

Swarm throughput is not the constraint (about 2 ms per seed on a GitHub runner, inference above). What limits evidence quality is workload shape: 5 s virtual horizon in brutal runs (`fuzz.rs:298-307`), at most 64 extra fault events (`fuzz.rs:18`), at most 40 client ops. Raising seed count 100x is nearly free; raising horizon and ops changes checker cost and needs the data in section 10, stage 4.

## 7. Trade-offs and alternatives

### A. Agent harness

Evidence that changes the earlier recommendation: record/replay at the HTTP layer already has several small Python tools (`pytest-llm-vcr`, `promptecho`, `Reel` as an HTTP proxy), with unresolved claims about VCR.py and SSE streaming ([search summary, 2026-11-03](https://pypi.org/project/pytest-llm-vcr/); I did not verify any of those projects). The Anthropic SDK accepts a custom base URL and `httpx` client (**not confirmed from the README I fetched; confirm in the SDK docs for the pinned version before building**).

| Option | For | Against |
|---|---|---|
| A1. Rust proxy + Python demo agent (my earlier proposal) | Shows Rust; framework-agnostic over HTTP | Needs an HTTP stack in a repo that bans dependencies; reimplements recorders that exist; SSE handling is the highest-risk piece; reusable Chronos code is only about 70 lines of ddmin and a SHA-256 digest |
| **A2. All-Python: httpx transport recorder in-process + simulated tools + ddmin** | Same language as the agents you build at work and as the likely interviewers; no new HTTP stack; shrinker is small; can adopt an existing recorder's design | Less "Rust"; ties to httpx-based SDKs (the Anthropic and OpenAI Python SDKs are believed to use httpx; *not verified here*) |
| A3. Use `pytest-llm-vcr` unmodified | Zero build | Does not give fault injection, invariants or trajectory shrinking, which is the differentiator |

**Choice: A2**, with the record/replay layer thin and the effort spent on tool-boundary faults, invariants, and shrinking. The Chronos link becomes methodological (mutation-validated oracles, seeded schedules, ddmin), documented rather than shared code. Keep it in a separate directory or repo so the Rust workspace and its determinism gates stay untouched. Effort drops: 2a (record/replay) from about 16 h to about 8 h (my estimate, unvalidated).

### B. Checker scaling

| Option | Decision |
|---|---|
| B0. Leave as is | **Chosen.** Real histories are low-concurrency and small; the 1000-seed run including the check takes about 2 s |
| B1. Per-key split | Valid and about 20 lines, but no current benefit; do it only when ops per run exceed about 48 |
| B2. Cap by concurrency width instead of op count | Better limit than 48 but needs measurement first |

Trigger to revisit: any plan to raise `MAX_CLIENT_OPS` above 48, or a `CheckerCapacity` failure in a run.

### C. Mutant mechanism

| Option | For | Against |
|---|---|---|
| C0. Keep the bool-per-hook pattern | Matches existing code | Does not scale to 6 to 8 hooks; leaves bug hooks in the production binary, as `skip_vote_persist` already is |
| C1. `#[cfg(test)]` | Matches the README's claim | Integration tests and the `chronos-sim` CLI are separate crates and cannot see `cfg(test)` of `chronos-protocol` |
| **C2. Cargo feature `mutants` on `chronos-protocol`, forwarded by `chronos-sim`; one `Option<Mutant>` field** | Production and default CI builds cannot contain the hooks; makes the README true; one field, one setter | Two build configs in CI; each `#[cfg(feature = "mutants")]` site must be reviewed so default behavior is byte-identical (verify with the existing seed digests) |

**Choice: C2.** Acceptance test for the refactor: with the feature on and no mutant selected, `--seed 7` and the 1000-seed run produce the same digests as without the feature.

## 8. Failure modes (prioritized)

1. **A mutant is silently undetected.** Likely for Figure 8: it needs a leader to replicate an old-term entry to a majority, crash, and a different leader to overwrite it, all inside a 5 s horizon and at most 64 extras. *Engineering inference, not executed.* Detection: report seeds-to-detect with a cap (100k) and list "not detected" as a finding. Mitigation: a targeted scripted schedule (like the planted test) proves the checker can catch it even if the swarm cannot.
2. **False credit from a leaky hook.** If a mutant leaks into the default build, CI would go red on clean code or, worse, go green with a weakened protocol. Mitigation: the digest-equality test above.
3. **Replay of agent runs hides side effects.** Replay never re-executes tools; a tool call outside the simulated world is not covered ([Tian Pan](https://tianpan.co/blog/2026-04-12-deterministic-replay-debugging-non-deterministic-ai-agents)). Mitigation: all tools run inside the simulated world; real network is blocked in replay mode.
4. **Streaming responses break the recorder.** Highest risk for A2 too. Mitigation: record non-streaming calls only in v1 and say so.
5. **Checker exponential blow-up** if the workload grows. Detection: `CheckerCapacity` or timeouts. Mitigation: B1/B2.

## 9. Recommended design (proposals, not existing behavior)

**C2 sketch.**

```rust
// chronos-protocol/src/raft/state.rs  (proposal)
#[cfg(feature = "mutants")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutant { SkipVotePersist, CommitOldTerm, NoLeaderNoop, AckBeforeFsync,
                  DropIdempotency, AcceptBadCrc, CommitRegress }
// Node gets `#[cfg(feature = "mutants")] pub(crate) mutant: Option<Mutant>` and one setter.
// Each call site: `#[cfg(feature = "mutants")] if node.mutant == Some(Mutant::CommitOldTerm) { ... }`
```

Keep `set_skip_vote_persist` as a thin wrapper while the planted test migrates, then delete it.

**A2 sketch (Python).** `Recorder` is an `httpx.BaseTransport` writing one JSON envelope per boundary (request hash, response bytes, virtual time, cost fields) with a run digest. `World` owns tool state and a seeded RNG; the fault schedule is a list of `(step, fault)` atoms so `ddmin(atoms, fails)` works on it directly. Invariants run after each step. Details of the invariants and planted agent bugs are in `NEXT_PHASES.md` Phase 2b.

## 10. Validation and roadmap

| Stage | Task | Acceptance | Reduces |
|---|---|---|---|
| 0 | Fix the build host: install MSVC Build Tools (or start Docker/WSL) and put MSVC `link.exe` ahead of Git's on PATH; confirm `cargo test --workspace --release` passes locally | Test counts match the CI log (101 sim lib tests, 43 protocol) | Every "not executed" below |
| 1 | Correct the README/HANDOFF claim about `skip_vote_persist` (or do C2 so it becomes true) | No doc claim that code contradicts | Credibility |
| 2 | C2 refactor, no mutants selected | Same digests as before for seed 7 and seeds 1..1000 | Leaky hooks |
| 3 | Add 6 mutants; run 100k seeds each (CI job) | Table of seeds-to-detect, or "not detected within 100k" | Unmeasured oracle power |
| 4 | Measure the checker: synthetic worst case (n concurrent Puts plus one impossible Get, n = 6..18, one key vs split) and max cache size across real seeds | Time and memory curve | Decision B revisit |
| 5 | A2 harness per `NEXT_PHASES.md` Phase 2, with the Nov 14 gate | Same criteria as there | Phase 2 risk |

I wrote the stage-4 synthetic test as a throwaway in the scratch copy (`C:\Users\sahil\AppData\Local\Temp\claude\C--projects-Chronos\556bab54-ccb3-4f12-9b4f-a4d0d57feb97\scratchpad\exp\crates\chronos-sim\tests\lin_scale.rs`) but could not run it. It uses only the public `History` API.

## 11. Evidence ledger (key claims)

| Claim | Type | Evidence | Confidence |
|---|---|---|---|
| `skip_vote_persist` is runtime, not `cfg(test)` | Source-code observation | `state.rs:101,140-143`, `election.rs:39,144`; the first `cfg(test)` in `election.rs` is line 253 | High |
| Swarm uses two keys | Source-code observation | `fuzz.rs:729-733` | High |
| Swarm runs about 2 ms/seed | Inference from two CI log timestamps | `gh run view 41463316257 --log` lines 661, 1672 | Medium (one run) |
| Checker is not the bottleneck | Inference | The above plus ≤40 ops per run | Medium; needs stage 4 |
| Figure 8 mutant likely hard to hit | Engineering inference | Reasoning on horizon and fault budget | Low; untested |
| Existing Python record/replay tools exist | Search summary, not verified | PyPI names above | Low to medium |
| Native build fails on this host | Tested result | `link: extra operand` from Git's `link.exe`; Docker pipe missing; WSL `E_UNEXPECTED` | High |

## 12. Research verdict

- **What we must understand:** the swarm is cheap, so evidence quality comes from workload shape and measured detection power, not from more checker engineering.
- **Recommended approach:** C2 (feature-gated `Mutant` enum), then a 100k-seed detection table, leave `history.rs` alone, and build the agent harness in Python with the effort on faults, invariants and shrinking.
- **Why:** each choice is the smallest change the evidence supports. The one place my earlier advice changed (Rust proxy) changed because existing Python recorders and the small amount of shareable Rust remove its justification.
- **Biggest trade-offs:** less Rust in the agent story; two CI build configs.
- **Biggest risks:** nothing here was executed; Figure 8 may go undetected; SSE streaming.
- **Avoid:** an HTTP proxy in Rust, generalizing `Scheduler`/`WorldEvent`, per-key checker work before it is needed, any further Raft feature work.
- **Next steps:** stage 0 first. Without a working build host every number in section 10 stays a plan.

---

## Appendix: ADR drafts (not yet written to `docs/adr/`; awaiting your approval)

### ADR-0001: Build the agent reliability harness in Python

**Date**: 2026-11-03 **Status**: proposed **Deciders**: Sahil

**Context.** Phase 2 needs record/replay, fault injection and trajectory shrinking for LLM agents. Chronos' reusable Rust is about 70 lines of ddmin and a digest. HTTP-level LLM recorders already exist in Python. Time budget is about 100 hours.

**Decision.** Build the harness in Python as a separate directory or repo: an httpx transport recorder, a seeded simulated tool world, invariants, and a ddmin shrinker. Chronos is referenced as method, not imported as code.

**Alternatives.** (1) Rust HTTP proxy plus Python demo agent: rejected, adds an HTTP stack and SSE risk for no shared code. (2) Use an existing recorder unmodified: rejected, lacks faults, invariants, shrinking.

**Consequences.** Positive: fits the stack your work and interviewers use; about 8 h less for record/replay. Negative: weaker Rust signal; coupled to httpx-based SDKs. Risks: streaming responses (record non-streaming only in v1).

### ADR-0002: Gate protocol mutants behind a Cargo feature

**Date**: 2026-11-03 **Status**: proposed **Deciders**: Sahil

**Context.** `skip_vote_persist` is a runtime field in the production crate despite docs calling it test-only. Phase 1 adds 6 to 8 more hooks.

**Decision.** Add feature `mutants` to `chronos-protocol` (forwarded by `chronos-sim`) holding a single `Option<Mutant>` on `Node`; default builds contain none of the hooks.

**Alternatives.** Bool per hook: does not scale and ships hooks. `#[cfg(test)]`: invisible to the sim CLI and integration tests.

**Consequences.** Positive: docs become true; one mechanism for all mutants. Negative: two CI configurations. Risk: a leaky `cfg` site; mitigated by a digest-equality check with the feature on and no mutant selected.
