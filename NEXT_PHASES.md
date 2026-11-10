# NEXT_PHASES.md — Chronos

Written 2026-11-03. Deadline given: end of November 2026, so **27 days / roughly 100 usable hours**, not months.

Tags used throughout: **[O]** observed in the repo, **[R]** found in web research (with source), **[I]** my inference. Anything I could not verify is marked *unverified*.

What I did not do: I did not build or run the code locally. The README says the Windows host has no MSVC `link.exe`, and building would write into `target/`. My evidence for "tests pass" is the GitHub Actions log of the 2026-11-02 nightly run (`gh run view 41463316257`), which shows the workspace tests green. I did not read all of `cluster.rs` (1,933 lines), `fuzz.rs` (1,557 lines) or `minify.rs` (1,030 lines) line by line.

---

## Status update, 2026-11-10 (supersedes the plan text below where they conflict)

- Phase 0, 1, 2 and 3 are implemented; see `INVESTIGATION.md` and `docs/adr/`. Phase 4 is partly done (README, demo command, blog draft, `docs/decisions.md` scaffold).
- **Phase 2 changed:** the harness is Python, not a Rust proxy (ADR-0001). Cut-point replay, SSE recording, OTel-style export and a real-model adapter exist; a live-model run has not been executed.
- **Phase 0 finding:** `skip_vote_persist` was never `cfg(test)`; it is now behind the `mutants` feature (ADR-0002).
- **Phase 1 result:** all six mutants detected, but four only at 100,000 seeds (README, "Oracle detection power"). The "within 1,000 seeds" target in Phase 1 was unrealistic.
- Still open: a live-model run (needs an API key and a spend cap), a first GitHub Actions run of the new jobs, your own answers in `docs/decisions.md`, and review of the 19 uncommitted doc-comment files.
- **Scope narrowed (later 2026-11-10):** Chronos is now "deterministic simulation testing with measured oracle detection power". Record/replay and real-model evals moved to Atlas (a separate repo), so the live-model item above no longer belongs here.
- **Workload experiment:** larger workloads do not catch the four rare mutants sooner (README, "Does a bigger workload catch the rare four sooner?"). The nightly ran the same seeds 1..1000 every night, so it caught four of six mutants and could not find new ones. Fixed: it now runs 100,000 seeds from `run_number * 100000` and logs the start seed (not yet run on GitHub).
- **G4 resolved by removal:** `docs/bugs/` stays gitignored and local; no tracked doc links to it (the mentions below are this plan's own analysis).

## 1. Project summary and honest assessment

**What it is [O].** A Rust workspace (about 11.6k lines, 3 crates, no third-party dependencies in `chronos-protocol` or `chronos-sim`). `chronos-protocol` is a pure Raft + KV state machine exposed as `step(node, event) -> Vec<Effect>`. `chronos-sim` is a single-threaded deterministic scheduler (heap keyed by `(time, global seq)`), seeded PRNG, simulated network and disk, 11 invariant checkers (`check.rs`, `CheckName`), a Wing-Gong style linearizability checker (`history.rs`), a swarm fuzzer (`fuzz.rs`) and a delta-debugging minimizer (`minify.rs`). `chronos-node` is a single-process WAL demo.

**Genuinely strong [O]:**
- The effect/completion boundary (`Effect::IoSubmit` plus `Event::IoComplete`, incarnation-tagged `IoId`) is a real design, not decoration. Persist-before-send and persist-before-ack are checked against a `PersistCookie` rather than against volatile state (`docs/02-architecture.md` D3, D17).
- Disk model separates torn tail (past `durable_len`) from fsync-lie, and the doc says why (D9, D10).
- Seed 281 was triaged as a *checker* false failure (`Io` treated as a definite miss) rather than being sold as a Raft bug (`history.rs:147`, test `io_put_then_get_same_value_is_linearizable`). That is the most interview-worthy story in the repo.
- CI runs fmt, release tests, clippy `-D warnings`, a determinism grep gate, and a nightly 1000-seed swarm. The last nightly runs I could see (2026-10-26 to 2026-11-02) all succeeded.

**Weak or hand-wavy [O]:**
- No real bug was ever found. The only bug artifact is a *planted* one, and the repo says so honestly. One planted bug (`skip_vote_persist`) is thin evidence that the oracles detect anything.
- The planted-bug pack (`docs/bugs/`), the roadmap and the HANDOFF are listed in `.gitignore`, so **the flagship artifact is not in the public repo**. Only `docs/00` to `03` are tracked (`git ls-files docs`). The README's "Harness proof" claim is verifiable only through a test in `crates/chronos-sim/tests/properties.rs`.
- The client workload is tiny: at most 40 client ops per run, 8 to 32 typical (`fuzz.rs:17`, `fuzz.rs:709-713`), and the linearizability checker hard-caps at 48 ops (`history.rs:13`). "1000 seeds" is 1000 very small runs. Brutal runs have a 5 s virtual horizon (`fuzz.rs:298-307`).
- The "coverage" report counts which fault types *fired* in a seed, not protocol states reached. It cannot say the swarm is exploring anything deep.
- `chronos-node` is mostly stubs: `net.rs` and `timer.rs` are 4-line placeholders, and `main.rs` runs a hard-coded Put/Get script. `kv/machine.rs` is 41 lines. There is no performance data anywhere.
- No snapshots, no membership change, no ReadIndex. Gets go through the log, which is correct but is the conservative choice.

**Where you would struggle in a grilling [O, then I].** `main`'s first three commits are all dated 2026-08-24 and the initial v1 landed as one commit (`ef4e7c7`, "Chronos v1"). `docs/HANDOFF.md:43-51` documents a Cursor agent workflow and local helper scripts that strip agent co-author trailers from commits. About 6,100 lines of local interview prep docs sit next to the code. **[I]** An interviewer who reads the git history will see a code dump, and the design docs (17 locked decisions, a 12-layer "determinism audit") are more polished than any one person's usual notes. You must be able to rederive each decision at a whiteboard (section 6). The uncommitted working-tree changes (790 lines across 28 files, mostly rustdoc comments like "Helper function to create a test `Put` client request event") are low-value agent-style comments. Review them before committing; they add noise to `git blame`, not understanding.

---

## 2. Alignment score vs. your core direction

**Overall: Medium.** Split by leg:

| Leg | Score | Why |
|---|---|---|
| Core backend (distributed systems, storage, HLD/LLD) | **High** | Raft, WAL with CRC, crash recovery, idempotent state machine, linearizability checking. These are exactly the foundations that durable-execution and event-sourcing agent systems sit on. |
| Agent reliability (evals + observability) | **Low as shipped** | Zero LLM content. No eval harness for model behavior, no traces of model calls, no cost or latency attribution. |
| Sandboxing | **None** | Nothing here. |

**Justification.** Chronos proves you can build the *machinery* that agent-reliability work needs (seeded replay, trace digest, failure minimization, an oracle that you can validate) but it applies that machinery to a protocol that startups in your target space do not build. The overlap is methodological, and it is real: replaying a failure exactly, shrinking it, and distrusting your own checker (seed 281) are the same problems agent-eval teams have. The distance is that none of it touches an LLM. **[I]** Left as is, the repo reads as "good distributed-systems student project"; extended carefully, it can be the evidence for "I know how to make non-deterministic systems reproducible and testable," which is the agent-reliability claim.

---

## 3. Research findings (key insights only)

Source quality is flagged. Several 2026 sources came from search snippets of secondary pages; where I fetched the page itself I say "fetched."

### Evals and observability

1. **Consistency, not just success rate, is the metric to report.** Anthropic's engineering post defines pass@k (at least one success in k tries) and pass^k (all k succeed), with a worked example: 75% per-trial success gives about 42% for pass^3. It says graders must be validated by reading transcripts, that a 0% pass rate across many trials usually means a broken task or grader, and that leftover state between trials causes correlated failures; Claude gained an unfair advantage in some internal evals by reading git history from earlier trials. — [Demystifying evals for AI agents](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents), 2026-01-09, **fetched**. The pass^k metric originates in τ-bench ([arXiv 2406.12045](https://export.arxiv.org/pdf/2406.12045), June 2024; numbers such as about 25% pass^8 on retail come from a search snippet, *unverified against the paper*) and [Sierra's write-up](https://sierra.ai/blog/benchmarking-ai-agents).
2. **LLM judges can be consistent and still biased.** A June 2026 study of 21 judges reports a "consistency-bias paradox": very high test-retest reliability alongside severe position bias, and says exact-match agreement overstates quality relative to chance-corrected measures. — [Reliability without Validity, arXiv 2606.19544](https://arxiv.org/abs/2606.19544), 2026-06, *abstract-level snippet only*. The MAST study found LLM-annotator agreement with experts of Cohen's κ 0.77 versus 0.88 between human annotators. — [Why Do Multi-Agent LLM Systems Fail?, arXiv 2503.13657](https://arxiv.org/abs/2503.13657), 2025-03 (arXiv v3, revised 2025-10-26: 1,600+ annotated traces, 150 used to develop the taxonomy; the abstract gives κ 0.88 for annotator agreement; the κ 0.77 LLM-judge figure above is **unverified** and came from a search snippet).
3. **Failure taxonomies exist but are research-grade.** MAST: 14 failure modes in 3 categories (specification, inter-agent misalignment, task verification), argued to be mostly system-design failures rather than model limits. Same source as above.
4. **OpenTelemetry GenAI conventions are still unstable.** Multiple sources say every `gen_ai.*` attribute is at "Development" status as of late 2026; agent spans (`invoke_agent`, `execute_tool`, `plan`) exist; the conventions moved to the `semantic-conventions-genai` repo on 2026-06-12 in v1.42.0, with no tagged release there (Dash0, fetched). A claim that agent spans were split in v1.41.0 came from a snippet and the fetched page does not support it. One June 2026 article claims stabilization; the others contradict it. — [Dash0, updated 2026-09-14](https://www.dash0.com/knowledge/opentelemetry-genai-semantic-conventions-explained), [Greptime, 2026-05-09](https://www.greptime.com/blogs/2026-05-09-opentelemetry-genai-semantic-conventions), [DEV Community](https://dev.to/mr_manushukla/opentelemetry-genai-conventions-zero-stable-genai-attributes-in-2026-and-what-to-ship-anyway-1l5h). *All secondary; check the spec repo before relying on attribute names.*
5. **Record-and-replay for agents is an active, young area.** The recipe: log every non-deterministic boundary (LLM response bytes, tool results, clock, RNG), replay by serving recorded values and failing loudly on an unrecorded call, never re-execute side effects. Temperature 0 does not give determinism. A 10-step run is roughly 100 to 200 KB. — [Tian Pan, 2026-04-12](https://tianpan.co/blog/2026-04-12-deterministic-replay-debugging-non-deterministic-ai-agents), **fetched** (a practitioner blog; its "15% accuracy variation" figure is cited from unnamed research). **Chronicle** adds *cut-point replay* (serve some boundaries from the record, run the rest live against new code), reports bit-stable replay across 20 repetitions and 23 µs recording overhead per boundary on **6 incidents with simulated model boundaries**. — [arXiv 2609.20625](https://arxiv.org/abs/2609.20625), 2026-09-17, **fetched (abstract)**. Earlier work: [AgentRR, arXiv 2505.17716](https://arxiv.org/pdf/2505.17716), 2025-05.
6. **Deterministic-simulation ideas are arriving in agent testing, but only as tiny projects.** `rouviere-german/deterministic-agent-simulator` (Python, MIT) implements a seeded fake world, fault injection, ten invariants, delta-debugging shrink (median 8.05× reduction, its own 10× target missed), a Wing-Gong checker. It has **0 stars, 0 forks, 22 commits**, a scripted (not real) LLM, and self-graded sensitivity results. — [GitHub](https://github.com/rouviere-german/deterministic-agent-simulator), **fetched 2026-11-03**. Antithesis's homepage lists posts on AI agents; I did not read them. **[I]** This is both validation (the idea is real) and a warning (a clone of it is not a differentiator).

### Sandboxing

7. **Isolation tiers are consistent across sources; the numbers are not.** MicroVM (Firecracker, Kata) is generally ranked strongest for untrusted code, gVisor mid, hardened containers weakest. E2B is described as Firecracker-based, Modal as gVisor. Cold-start figures conflict between sources (E2B about 150 ms in several, 5 to 30 ms in one). — [Northflank](https://northflank.com/blog/how-to-sandbox-ai-agents), [DEV Community](https://dev.to/chunxiaoxx/mcp-security-patterns-2026-gvisor-vs-firecracker-for-ai-agent-sandboxing-3hp7), vendor-adjacent, 2026. *Treat benchmarks there as marketing.*
8. **Default Docker is not a boundary against capable models.** SandboxEscapeBench (UK AISI-linked authors) is an Inspect CTF benchmark; its abstract states that when vulnerabilities are added to the environment, LLMs can identify and exploit them. The benchmark itself uses a nested sandbox design to run safely; the abstract does **not** recommend nested sandboxing as a production defense, and the "default Docker with common misconfigurations" detail came from a secondary summary I could not confirm. — [arXiv 2603.02277](https://arxiv.org/abs/2603.02277), submitted 2026-03-01 (v3 2026-08-01), abstract fetched.
9. **Anthropic open-sourced the Claude Code sandbox** (`@anthropic-ai/sandbox-runtime`, Apache-2.0): Seatbelt on macOS, bubblewrap on Linux/WSL2, proxy-based network filtering, deny-by-default network. — [Claude Code docs](https://docs.claude.com/en/docs/claude-code/sandboxing), [AI Weekly](https://aiweekly.co/alerts/anthropic-open-sources-claude-code-sandbox-under-apache-20). *I did not retrieve the repo README or config schema.*
10. **Prompt-injection containment is about constraining capability, not filtering text.** The lethal trifecta (private data + untrusted content + external communication) and the six design patterns paper (dual LLM, code-then-execute, map-reduce, etc.) argue that after an agent ingests untrusted input it must be unable to trigger consequential actions. — [Simon Willison, 2025-06-13](https://simonwillison.net/2025/Jun/13/prompt-injection-design-patterns/), secondary summary; primary papers not read.

### Backend foundations

11. **Durable execution is mainstreaming for agents.** The OpenAI Agents SDK + Temporal integration reached GA on 2026-03-23 per Temporal. The claim that OpenAI's Codex web agent runs on Temporal is **unverified**: it came from a search snippet and the Temporal integration post I fetched makes no Codex claim. Durable-execution systems journal non-deterministic operations (LLM calls, API requests) and replay the journal on recovery, which is the same idea as the record/replay in finding 5. — [Temporal blog](https://temporal.io/blog/announcing-openai-agents-sdk-integration), [Temporal sandbox post](https://temporal.io/blog/introducing-temporal-and-agentic-sandboxes-openai-agents-sdk), [InfoQ](https://www.infoq.com/news/2025/09/temporal-aiagent). Funding figures for Temporal differ across the sources I saw ($300M Feb 2026 vs $550M Sep 2026), so I do not use them.

### Market and hiring (thin evidence; I found no first-hand interview reports)

12. **Money is in this space.** Braintrust: $80M Series B led by Iconiq, reported $800M valuation, 2026-02-17 ([SiliconANGLE](https://siliconangle.com/2026/02/17/braintrust-lands-80m-series-b-funding-round-become-observability-layer-ai), [Axios Pro](https://www.axios.com/pro/enterprise-software-deals/2026/02/17/ai-observability-braintrust-80-million-800-million)). ClickHouse acquired Langfuse, 2026-01-16 ([ClickHouse](https://clickhouse.com/blog/clickhouse-acquires-langfuse-open-source-llm-observability)). Antithesis: $105M Series A led by Jane Street, 2025-12-03 ([PR Newswire](https://tools.prnewswire.com/en-us/live/20823/release/20251203EN37581)). Daytona: $24M Series A (Feb 2026), later $60M Series B at $1B (one source dates it 2026-10-20). Modal: $355M Series C at $4.65B (May 2026). E2B: $21M Series A (July 2025, no 2026 round found). ([Dealroom](https://dealroom.co/news/164304-daytona-raises-60m-series-b-at-1b-valuation-to-build-the-agentic-cloud/), [Built In](https://builtin.com/articles/e2b-raises-21m-series-a-20250730)). Sandboxes and eval/observability are both funded categories.
13. **Postings that match your direction** (aggregator-sourced; funding stage of the employers not verified; I could not re-open the Town page, it returned 403, so treat all listings as unconfirmed): Town, "AI Engineer, Evals & Agent Quality", owns multi-step trajectory evaluation and regression prevention, dated 2026-09-27 ([listing](https://www.jobleads.com/us/job/ai-engineer-evals-agent-quality--san-francisco--e93e6654d0f91c3856dd34623d7e909b8)). LangChain, "Software Engineering Manager, AI Observability & Evals Platform", 2026-08-19 ([listing](https://zapply.jobs/jobs/621dc66f-475f-40c1-b76f-13d64b092aa7/)). Glean, "ML Engineer, LLM Evals & Observability", $200k to $300k, March 2026 (not an early-stage company) ([listing](https://www.indeed.com/viewjob?jk=0625640913127e10)). Antithesis hires Rust engineers on its open-source testing project Bombadil ([listing](https://startup.jobs/senior-staff-engineer-bombadil-antithesis-8133900)).
14. **Deterministic simulation in Rust is already a conference topic.** FOSDEM 2026 and QCon London (March 2026) talks; SurrealDB wrote about it on 2026-06-19; Polar Signals, RisingWave, S2 also use it ([SurrealDB](https://surrealdb.com/blog/deterministic-simulation-testing-and-its-use-at-surrealdb), [FOSDEM](https://fosdem.org/2026/schedule/event/rust-deterministic-simulation-testing)). **[I]** A Raft-under-DST repo, by itself, will not stand out at DST-literate companies; it needs a differentiator.

### Evidence that is thin
- Interview reports: my searches returned job postings and prep guides, not first-hand accounts. I have nothing citable about the questions these startups ask. Section 6 is therefore my judgment, not reported data.
- Cost and latency attribution for agents: I did not research this separately, so I make no claim about the state of the art.
- Production adoption of record/replay in agent platforms (as opposed to papers and blogs): not established by what I found.

---

## 4. Gaps and opportunities specific to this repo

**G1. Nothing connects Chronos to LLM behavior [O].** The most reusable parts are the seeded RNG (`rng.rs`), the `(time, seq)` scheduler (`scheduler.rs`), the SHA-256 trace digest (`trace.rs`), and the delta-debugging loop (`minify.rs:512-585`). **[I]** They are coupled to Raft types: `WorldEvent` in `scheduler.rs:11` hard-codes `NodeId`, `Message`, `IoId`, and `minify.rs:9-11` imports `Cluster`, `ReplayBook`. So reuse means *copying and generalizing* about 300 lines into a new crate, not importing them. Do **not** refactor the working Raft sim to be generic; that risks the 1000-seed green bar for no interview payoff.

**G2. The oracle's detection power is unmeasured [O].** One planted bug. For an eval-focused interviewer the natural question is "how do you know your checker catches things and doesn't cry wolf?" You have a good false-positive story (seed 281) and almost no false-negative evidence. A mutation suite closes this and mirrors "validate your graders" (finding 1).

**G3. Scale of the evidence is small [O].** ≤40 ops per run, 48-op linearizability cap, no per-key decomposition. Fine for a v1, but it caps any claim you can make about the checker.

**G4. The artifact trail is missing from the public repo [O].** `docs/bugs/` is gitignored. Either un-ignore the planted pack or stop linking to it.

**G5. No observability layer [O].** The only "telemetry" is the trace digest and a coverage table. For the agent leg, the equivalent (per-step trace with cost/latency, exportable as spans) does not exist.

**G6. Open problem you can credibly occupy [I, from findings 5 and 6].** Existing agent replay work is either a design article, a small-benchmark paper with simulated model boundaries (Chronicle), or a scripted-LLM toy (deterministic-agent-simulator). I did not find a project that (a) records real LLM runs through a framework-agnostic proxy, (b) injects distributed-systems faults at the tool boundary, (c) shrinks the failing *trajectory*, and (d) reports pass^k per fault profile. That is a single-search conclusion; check GitHub again before you commit to this claim in public.

**Leave alone / deprioritize:**
- Raft features (snapshots, membership change, ReadIndex, TCP multi-node `chronos-node`). Each is weeks of work and moves you toward the "another distributed-systems toy" you said you do not want.
- Fsync-lie and PAR, Jepsen, Antithesis integration: all listed non-goals in the README. Keep them as non-goals.
- Perf tuning of `chronos-node`.
- More interview-prep markdown. You have 6,100 lines; the gap is understanding, not documents.

---

## 5. Proposed next phases

Ordered by impact per hour. Dependencies and a calendar are at the end of this section.

### Phase 0 — Make the public repo honest (6 h)

**Goal.** An interviewer who opens the GitHub repo can verify every claim in 10 minutes.

**Deliverables**
- Decide on the working-tree diff: read it, keep only comments you can defend, commit or `git restore`. (I did not touch git; this is your call.)
- Track the planted-bug pack: remove `docs/bugs/` from `.gitignore` or copy the pack to a tracked path; keep `HANDOFF.md`, interview docs and `rustup-init.exe` ignored.
- Add one CI line that runs the two planted-regression tests by name, so "Harness proof" is checked on every push.
- Replace the README claim "same SHA-256 digest across runs on the same commit" with exactly what CI proves. If you want to claim Windows = Linux digest equality (the architecture doc says so, D13), add a CI matrix job on `windows-latest` that compares `--seed 7` digests, or drop the claim. *I found no evidence in CI that cross-OS equality is tested.*
- README: add a "Limitations" paragraph stating the ≤40-op workload and 48-op checker cap.

**Acceptance.** `git ls-files docs/bugs` is non-empty; CI runs the planted tests; README has no claim CI does not check.

**Effort.** 6 h. **Risks/cut.** The Windows matrix job can be cut if the linker is a problem; then delete the claim.

### Phase 1 — Validate the oracles with a mutation suite (14 h)

**Goal.** Turn "the checkers work" into a measured detection rate. This is the Chronos-side proof of "evaluate your evaluator," and it is the story that bridges to agent evals.

**Deliverables**
- 6 to 8 planted protocol mutants behind a Cargo feature (not `#[cfg(test)]`, so the CLI and swarm can run them), each a known Raft bug class. Candidate list tied to your code: commit by counting replicas for an old-term entry (`CurrentTermCommit` check, `raft/replication.rs`); skip the leader no-op; ack `AppendEntries` before the fsync completes (`PersistBeforeAck`); serve `Get` from leader memory (violates D7, should trip `Linearizability`); drop the idempotency lookup in `kv/machine.rs:apply`; accept a WAL record with a bad CRC in `wal/record.rs` `scan`; let `commitIndex` regress after recovery. The existing `skip_vote_persist` is mutant #1.
- A table: mutant, check that fired, seeds-to-first-failure (median over 20 start offsets), minimized schedule length from `--minify`.
- A `CHECKER`-class false-positive control: 1,000 seeds on the unmutated code, 0 failures (you already have this; cite it).
- Optional (3 h): per-key decomposition in `history.rs` (P-compositionality) to raise the op cap, but only if the workload actually uses several keys. *I did not check the key distribution in `fuzz.rs`.*

**Acceptance.** Done: ≥6 mutants, each detected by the check you predicted or a documented different one. Impressive: ≥5 of 6 detected within 1,000 seeds and the median minimized schedule is reported with before/after sizes. If a mutant is *not* detected, that is a finding: write it up as a checker gap rather than hiding it.

**Effort.** 14 h (8 h mutants, 3 h measurement harness, 3 h table and write-up). **Depends on** Phase 0. **Risks/cut.** If a mutant needs invasive changes to `step.rs`, drop it. Floor is 4 mutants.

### Phase 2 — `chronos-agent`: record/replay, fault injection and trajectory shrinking for tool-using LLM agents (46 h)

**Goal.** A new workspace crate that applies the Chronos method to an LLM agent: make a failing agent run reproducible, make it small, and measure reliability under faults. This is the phase that makes the repo serve **agent reliability** and gives the backend leg a reason to exist.

**Design decision (needs your sign-off, my recommendation first).**
- *Recommended:* Rust CLI plus a small HTTP record/replay proxy in front of the model API, a simulated tool world with seeded faults, and a ~150-line Python demo agent using the Anthropic SDK pointed at the proxy's base URL. Framework-agnostic (works with any agent that uses an HTTP model API), shows Rust depth, reuses the Chronos digest and ddmin ideas.
- *Alternative:* all-Python harness (faster to write, easier to attach to your Nexus/ADLC-style agents, but you lose the shared-code story and it looks like the 0-star Python project in finding 6).
- Constraint: a proxy needs an HTTP stack, which breaks the zero-dependency rule. Put it in the new crate only; `chronos-protocol` and `chronos-sim` stay dependency-free and the determinism gates stay unchanged.

**Sub-phases**

*2a. Boundary recording and replay (16 h).*
- Envelope format (content-addressed JSONL): one record per non-deterministic boundary (model request/response bytes, tool request/response, virtual clock reads), with a SHA-256 run digest in the same style as `trace.rs`.
- `record` and `replay` modes. Replay makes **zero live model calls** and fails loudly on an unrecorded request (finding 5).
- Cut-point replay: serve selected boundaries from the record and run the others live. Note Chronicle already published this idea (finding 5); credit it and differentiate on the real-model runs in 2c.
- Redact secrets at record time (API keys, auth headers) and test it.

*2b. Simulated tool world with fault injection and invariants (20 h).*
- Three toy tools with a state model, deliberately mirroring distributed-systems pain: a payments-like tool with idempotency keys, a ticket-like tool with read-after-write lag, a file or code-exec tool.
- Seeded faults at the tool boundary: timeout *after* commit, duplicate delivery, 429/5xx with retry-after, truncated response, crash and resume mid-run, stale read. These map directly onto what seed 281 and the idempotency table already taught you.
- Invariants checked after every step (the agent analogue of `check.rs`): no duplicate externally visible effect for one logical request, spend and step budget respected, no tool call outside granted scope, no privileged call whose arguments derive from untrusted tool output.
- Delta-debugging of the *fault schedule and trajectory* (generalize `minify.rs:512-585`; copy, do not refactor).
- 5 planted agent bugs for a detection table (same logic as Phase 1): retry without idempotency key; swallowed tool error leading to a claimed success; unbounded retry loop; stale cache after a timeout; privileged call triggered by injected text in a tool result.

*2c. Reliability report on a real model (10 h).*
- Run a small task set (10 to 15 tasks) with a cheap current model across 3 fault profiles (none, mild, brutal), N trials each, and report pass^1 and pass^k with the formula from finding 1, plus per-step cost and latency (token counts and wall time recorded in the envelope) with a "where did the dollars and the p95 go" breakdown by step type.
- For every failing trial: replay it with zero model calls and confirm the same digest; shrink it; report original vs shrunk step count.
- Set a hard spend cap before you start and record actual cost. I have no verified token price figures, so look them up and write the number into the report.
- Optional (4 h, cut first): export a replayed run as OpenTelemetry GenAI spans. Attribute names are in Development status (finding 4), so pin the version you target and state it in the README.

**Acceptance criteria**
- Done: replay of any recorded run produces the identical digest in 20 consecutive repetitions with 0 live model calls; ≥5 fault classes; ≥4 invariants; ≥4 of 5 planted agent bugs detected, 0 invariant failures on the bug-free agent over ≥500 seeds; shrinker works end to end.
- Impressive: median trajectory reduction ≥5× on **real recorded failures** (the 0-star competitor reports 8.05× on a scripted agent, so a real-model number is the differentiator); pass^k table with confidence intervals; a clearly written section listing what the harness cannot do (it cannot prove a fix works, because a changed prompt changes model outputs; replay shows what happened, not why, per finding 5).
- Measure and publish record overhead per boundary instead of quoting Chronicle's 23 µs.

**Effort.** 46 h. **Depends on** Phase 0 for repo hygiene; independent of Phase 1 technically, but schedule it after so the mutation-table format is reused.

**Risks and cuts.**
- Highest risk: the HTTP proxy and streaming (SSE) responses. Mitigation: record non-streaming calls only in v1, state it as a limitation.
- Second risk: the real-model run costs more than expected. Mitigation: spend cap, smaller task set.
- Cut order if short on time: OTel export, then cut-point replay (keep full replay), then the third tool, then reduce planted bugs from 5 to 3. **Do not cut** the shrinker or the real-model run; they are the differentiators.
- **Hard gate: if 2a is not working by Nov 14, stop Phase 2 and go to the minimum viable path below.**

### Phase 3 — Sandboxed code-exec tool, with containment tests (16 h, optional)

**Goal.** Cover the sandboxing leg with measured evidence, not a diagram.

**Deliverables**
- Route the file/code tool from 2b through a real boundary. The Anthropic sandbox-runtime README now documents native Windows support (dedicated user, Windows Filtering Platform egress fence, write-restricted token; beta research preview with listed limitations), so WSL/Docker is not required; whether it works on your Windows edition is untested. Firecracker needs KVM, so treat it as read-and-explain only unless you have a Linux host (this machine reports a hypervisor present, but WSL and Docker are currently broken).
- A containment test table: egress to a non-allowlisted host blocked; write outside the workspace blocked; fork/memory limit enforced; secrets not visible in the environment; an injected-instruction scenario where the capability policy (not text filtering) prevents the exfiltration (finding 10). Each row: attack, expected, observed.
- A one-page threat model that says what the sandbox does **not** protect against (finding 8: container escape on misconfigured Docker).

**Acceptance.** ≥6 containment tests with pass/fail results from an actual run, plus the threat model. **Effort** 16 h. **Depends on** 2b. **Cut:** the whole phase; it is the first thing to drop.

### Phase 4 — Evidence packaging (14 h)

**Deliverables**
- Rewrite the README top section: Chronos is a deterministic-simulation toolkit; Raft is the first target, agents are the second. Put numbers from Phase 1 and 2c at the top, limitations next.
- One 3-minute terminal demo (see section 7) recorded as a GIF or asciinema.
- One blog post (section 7).
- A `docs/decisions.md` where *you* explain in your own words 10 decisions (section 6 list) in under 5 lines each. Writing these is the real test of whether you can defend them.

**Acceptance.** A reader can reproduce the headline numbers with one command per table. **Effort** 14 h. **Cut:** the GIF.

### Order, dependencies, calendar

Dependencies: P0 → P1 → P2a → P2b → P2c → P4; P3 branches off P2b.

| Dates (2026) | Work | Hours |
|---|---|---|
| Nov 3 to 5 | P0 | 6 |
| Nov 6 to 11 | P1 | 14 |
| Nov 12 to 14 | P2a, **gate on Nov 14** | 16 |
| Nov 15 to 21 | P2b | 20 |
| Nov 22 to 26 | P2c | 10 |
| Nov 27 to 30 | P4 | 14 |
| only if ahead | P3 | 16 |
| **Total without P3** | | **80 h**, leaving about 20 to 40 h of slack against roughly 100 to 120 usable hours |

(This assumes 4 to 5 focused hours a day; the slack is deliberate because my effort estimates are unvalidated guesses for work I have not seen you do.)

### Minimum viable path (if Phase 2 slips)

P0 + P1 + a write-up: **about 30 h**. You end November with a fixed-up public repo, a measured mutation-detection table, and the seed-281 story rewritten as "I validated my oracle both ways." That is weaker on the agent-reliability axis; to compensate, add 6 h to build only 2c's *reporting piece on your existing work agents* (pass^k and cost/latency breakdown over your Nexus-style gold-standard eval set), written up generically without proprietary data. Check with your employer's rules before publishing anything derived from work code or data.

---

## 6. Interview-defensibility

**Ten hardest questions** on this repo, why they bite, and what to study. Starred items are where I judge your understanding is probably shallow **[I]**, based on the agent-assisted history in section 1 and on what the code does not do.

1. **"Why must `RequestVote` wait for the term/votedFor fsync? Walk me through the crash that breaks it."** Know the double-vote scenario and why your D3 says vote persist gates the *send*, not the whole batch. Study: Raft paper §5.2 and Figure 2; the sofa-jraft bug in `docs/03-research.md` (PR 1245).
2. **"Show me why Figure 8 needs the leader no-op and the current-term commit rule."** Be able to draw the 5-node scenario unaided. Study: Raft paper §5.4.2, Ongaro's thesis ch. 3 and §6.4.
3. *** "Your Get goes through the log. How would you do linearizable reads without it, and what breaks with leader leases and clock drift?"** Study: Ongaro thesis §6.4 (ReadIndex, lease reads), and Kingsbury's Jepsen writeups on stale reads. You do not implement either, so you need the reasoning in your head.
4. *** "Explain your linearizability checker's complexity and its cap."** Checking is NP-complete in general; `history.rs` is Wing-Gong with memoization on `(remaining-bitmask, state)` and clones a `BTreeMap` per node. Why 48 ops, and what does P-compositionality (per-key decomposition) buy? Study: Herlihy and Wing 1990; Gibbons and Korach 1997; Lowe, "Testing for linearizability"; Horn and Kroening, "Faster linearizability checking via P-compositionality"; the Porcupine implementation notes.
5. **"Seed 281: how did you know the checker was wrong and the protocol was right?"** You need the argument, not the outcome: why `Io` after a failed fsync is *indeterminate* (the entry may be in the log and commit later), why `NotLeader` is definite. This is your best story; rehearse it with the actual trace in `traces/fail-281.*`.
6. *** "The sim says green. Why should I believe the sim isn't wrong?"** FDB's "nightmare case." Your honest answer today: one planted bug, coverage counts fault types fired, small workloads. Phase 1 gives you a real answer. Study: the FDB paper (SIGMOD 2021) section on simulation; "Swarm testing" (Groce et al. 2012).
7. **"What exactly guarantees `same seed → same digest`, and what could silently break it?"** BTreeMap ordering, no wall clock, one RNG, global seq. What about a compiler or std upgrade, `f64`, or a dependency you add in a new crate? The architecture doc claims Windows = Linux digests (D13) and I found no CI job testing it, so do not state it as fact until tested.
8. *** "What does your minimizer guarantee?"** ddmin gives 1-minimality (no single removal still fails), not global minimality. Why `ReplayBook` exists (a filtered schedule changes later PRNG draws, so you record choices instead of re-deriving). No reduction ratios are published in the repo. Study: Zeller and Hildebrandt, "Simplifying and isolating failure-inducing input" (2002).
9. *** "What storage faults does your WAL survive, and which does Raft fundamentally not survive?"** Torn tail past `durable_len`: yes. Fsync-lie and bitrot inside the durable prefix: you turned them off. Why vanilla Raft can lose committed data on those; what Protocol-Aware Recovery does. Study: Alagappan et al., "Protocol-Aware Recovery for Consensus-Based Storage" (FAST 2018); Rebello et al., "Can Applications Recover from fsync Failures?" (USENIX ATC 2020).
10. **"What is the idempotency table's lifetime, and what happens when you add snapshots?"** It is unbounded state-machine state today (`kv/machine.rs`, D8). Real systems expire client sessions (Raft thesis ch. 6) and snapshots must include the table. Also: why TLA+/Stateright model checking and simulation catch different bug classes, and why Chronos can say little about liveness.

**If you build Phase 2, add:** "How do you replay a run when the model response differs on re-run?" (you do not call the model; explain why a fix cannot be validated by replay); "How is your pass^k estimator affected by correlated trial failures?" (finding 1); "Why not just temperature 0?" (finding 5); "What stops a prompt-injected tool result from becoming a privileged call?" (finding 10). Study: Anthropic's evals post, the τ-bench paper, Chronicle, and Temporal's durable-execution docs for the comparison "your journal vs their event history."

**Where I judge understanding likely shallow [I]:** the checker algorithm internals (4), the safety argument behind the minimizer (8), the storage-fault literature (9), and anything about reads and clocks (3), because the repo implements the conservative option in each. This is an inference from how the repo was built and what it leaves out, not a test of your knowledge.

---

## 7. Resume and portfolio angle

**Bullets you can write today (all [O]):**
- Built a deterministic-simulation-tested Raft key-value store in Rust (about 11.6k LOC, no runtime dependencies in the protocol and simulator crates): seeded replay with a SHA-256 trace digest, 11 invariant checkers plus a linearizability oracle, a delta-debugging schedule minimizer, and a nightly 1,000-seed CI swarm.
- Diagnosed a linearizability failure (seed 281) as a checker defect rather than a protocol bug by replaying the digest-identified trace; corrected the oracle's treatment of indeterminate (`Io`) responses and re-verified 1,000 seeds clean.

**Bullets to write only after you measure (placeholders must be filled with real numbers or deleted):**
- Extended the harness to LLM agents: record/replay proxy with zero-model-call replay (identical digest across 20 repeats), seeded tool-boundary fault injection, and trajectory shrinking (median [X]× reduction on [N] real failing runs).
- Validated detection power with [K] planted bugs in the protocol and [M] in the agent; [a] of [K] and [b] of [M] detected, 0 false positives over [S] seeds. Reported pass^[k] under 3 fault profiles at $[C] total spend.

**Demo (3 minutes, terminal).** Run an agent against the simulated payments tool at seed 4242. A timeout-after-commit plus a naive retry produces a double charge (invariant "no duplicate effect" fails). Show the 100+ step trajectory, run the shrinker down to a handful of steps, replay it with the network cable unplugged (no model calls, same digest), then show the generated regression test going red on the buggy agent and green on the fix. Say out loud what it cannot do.

**Blog post.** "Your agent's retry is a distributed-systems bug: reproducing double effects with seeded simulation." Content: the Raft idempotency-table lesson (seed 281, D8) → the same failure class at the agent tool boundary → numbers from Phase 2c → limitations. Anchor it to Anthropic's pass^k framing and credit Chronicle and the deterministic-agent-simulator as prior art. If you can write only one thing, write this.

---

## 8. Open-source angle

Realistic, in order of signal per hour **[I]**:
1. **Publish the generalized pieces as a small crate or repo**: the ddmin shrinker + seeded fault schedule from Phase 2 as a reusable library with docs. Low risk, and it is your own work. *I did not check whether a comparable Rust crate already exists; search crates.io first.*
2. **Contribute to the Inspect AI sandbox ecosystem** (UK AISI's `inspect-k8s-sandbox`, Meridian Labs' `inspect-sandboxes`) after Phase 3: sandbox correctness and per-trial isolation bugs are exactly the "correlated failures" Anthropic describes in finding 1. Look at open issues before planning; I did not.
3. **Open an issue or proposal on the OpenTelemetry GenAI semantic conventions** if Phase 2's replay needs attributes that do not exist (for example a boundary-record identifier tying a span to a recorded envelope). The spec is in Development status (finding 4), so well-argued input is more plausible to land than on a stable spec. Speculative: I did not read the contribution process.
4. **Antithesis's open-source Bombadil** (Rust, hiring signal in finding 13): a bug fix or test there is a direct line to a funded DST company. I did not evaluate the repo's contributor-friendliness.

Not recommended: patching the 0-star Python simulator (low signal), or adding features to MadSim/Turmoil (large, mature codebases, not tied to your agent angle).

---

## 9. Recommendation

**Sharpen, time-boxed. Freeze the Raft core.**

Reasoning:
- As-is, Chronos is a solid backend credential and a weak agent-reliability credential. Further Raft work has near-zero marginal value for your targets and is exactly the "another distributed-systems toy" direction you rejected.
- The methodology is the transferable asset, and the market evidence supports it: eval/observability and sandboxing companies are being funded now (finding 12), DST is on conference stages (finding 14), and replay for agents is being written about but not yet owned by a credible project (findings 5, 6).
- The risk is real: Phase 2 is 46 hours of new work with a hard gate on Nov 14. If the gate fails, the fallback (P0 + P1 + write-up) still leaves the repo better than it is now, and you should then put remaining time into rehearsing section 6 and your work projects, not into more Chronos code.
- The 27-day window is the binding constraint. One deep, measured deliverable beats three half-finished ones.

Not recommended: dropping the repo. It is the only project of yours with objective, checkable correctness evidence, and the seed-281 story is a better answer to "tell me about debugging an evaluation/oracle problem" than most candidates have.
