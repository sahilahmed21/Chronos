# chronos_agent

Seeded fault injection, record/replay and failure shrinking for tool-using agents. Same method as the Raft simulator in the parent repo (seed, trace digest, delta debugging), applied at the agent's tool boundary. Design decision: [ADR-0001](../docs/adr/0001-agent-harness-in-python.md). Sandbox threat model: [docs/threat-model.md](../docs/threat-model.md).

```bash
cd agent
PYTHONPATH=. uv run --no-project --with pytest --with httpx python -m pytest tests -q
PYTHONPATH=. uv run --no-project --with httpx python -m chronos_agent demo
PYTHONPATH=. uv run --no-project --with httpx python -m chronos_agent detect --seeds 2000
PYTHONPATH=. uv run --no-project --with httpx python -m chronos_agent shrink --agent retry_no_idem --emit-test
PYTHONPATH=. uv run --no-project --with httpx python -m chronos_agent eval --agent good --seeds 1000
```

| Module | Role |
|---|---|
| `world.py` | Simulated tools (`charge`, `create_ticket`, `find_ticket`, `read_notes`); 7 fault kinds keyed by call index; invariants checked on every call and at finish |
| `agents.py` | `good` reference agent and 5 planted bugs |
| `llm_agent.py` | Messages-API tool-use loop that drives the same world with a real model |
| `shrink.py` | ddmin over the fault schedule; emits a pytest regression |
| `recorder.py` | httpx transport: record/replay model calls (SSE stored whole), cut-point replay, secrets never stored, per-step token/latency/cost attribution |
| `otel.py` | Recorded calls as OpenTelemetry-style GenAI span dicts (`gen_ai.*` is Development-stability) |
| `sandbox.py` | bubblewrap command sandbox with resource limits (Linux) |
| `report.py` | pass^k estimator and Wilson interval |

## Invariants

`duplicate_effect`, `budget`, `step_budget`, `scope`, `tainted_privileged_call` (privileged call driven by tool output), `false_success` (claimed success without the effect).

## Measured (executed, 2026-11-10; scripted agents, not a real model)

- 2000 brutal seeds: 0 false positives for `good`; all 5 planted bugs detected (first seeds 9, 2, 4, 11, 1).
- Shrinking over seeds 1..399: median fault schedule 5 faults reduced to 1.
- Success over 1000 fault schedules (`eval`): `good` pass^1 1.000 / 0.930 / 0.752 for none / mild / brutal, pass^5 1.000 / 0.695 / 0.240. `retry_no_idem` brutal: pass^1 0.515, pass^5 0.036. The `good` agent's failures are bounded retries giving up during outages, not invariant violations.
- Sandbox: 8 containment tests pass under WSL2 + bubblewrap (see the threat model).

## Limitations

- Agents here are deterministic functions, so `pass^k` measures consistency across fault draws, not model sampling variance.
- `llm_agent` and `Recorder` are tested against scripted `httpx.MockTransport` only. **No live model run has been executed**; the cost and latency attribution and any real pass^k need one (`llm_agent(httpx.Client(transport=Recorder(path, "record", httpx.HTTPTransport())), model, api_key)`).
- SSE responses are recorded whole, so chunk boundaries and incremental timing are lost.
- The off-task-amount heuristic in `llm_agent` (any charge other than 2000 counts as tool-driven) is specific to this task.
- Sandbox tests skip where bubblewrap or unprivileged user namespaces are unavailable (including Windows and some hardened CI runners).
- Replay shows what happened, not why; it cannot validate a prompt or model change.
