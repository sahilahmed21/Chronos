# chronos_agent

Seeded fault injection and failure shrinking for tool-using agents. Same method as the Raft simulator in the parent repo (seed, trace digest, delta debugging), applied at the agent's tool boundary. Design decision: [ADR-0001](../docs/adr/0001-agent-harness-in-python.md). Record/replay of model calls and real-model evals live in Atlas, a separate repo, not here. Sandbox threat model: [docs/threat-model.md](../docs/threat-model.md).

```bash
cd agent
PYTHONPATH=. uv run --no-project --with pytest python -m pytest tests -q --ignore=tests/experimental
PYTHONPATH=. uv run --no-project --with pytest python -m pytest tests/experimental -q   # needs Linux + bubblewrap
PYTHONPATH=. uv run --no-project python -m chronos_agent demo
PYTHONPATH=. uv run --no-project python -m chronos_agent detect --seeds 2000
PYTHONPATH=. uv run --no-project python -m chronos_agent shrink --agent retry_no_idem --emit-test
PYTHONPATH=. uv run --no-project python -m chronos_agent eval --agent good --seeds 1000
```

| Module | Role |
|---|---|
| `world.py` | Simulated tools (`charge`, `create_ticket`, `find_ticket`, `read_notes`); 7 fault kinds keyed by call index; invariants checked on every call and at finish |
| `agents.py` | `good` reference agent and 5 planted bugs |
| `shrink.py` | ddmin over the fault schedule; emits a pytest regression |
| `experimental/sandbox.py` | **Experimental.** bubblewrap command sandbox with resource limits (Linux); see the threat model. Not part of the headline. |
| `report.py` | pass^k estimator and Wilson interval |

## Invariants

`duplicate_effect`, `budget`, `step_budget`, `scope`, `tainted_privileged_call` (privileged call driven by tool output), `false_success` (claimed success without the effect).

## Measured (executed, 2026-11-10; scripted agents, not a real model)

- 2000 brutal seeds: 0 false positives for `good`; all 5 planted bugs detected (first seeds 9, 2, 4, 11, 1).
- Shrinking over seeds 1..399: median fault schedule 5 faults reduced to 1.
- Success over 1000 fault schedules (`eval`): `good` pass^1 1.000 / 0.930 / 0.752 for none / mild / brutal, pass^5 1.000 / 0.695 / 0.240. `retry_no_idem` brutal: pass^1 0.515, pass^5 0.036. The `good` agent's failures are bounded retries giving up during outages, not invariant violations.
- Sandbox: 8 containment tests pass under WSL2 + bubblewrap (see the threat model).

## Limitations

- Agents here are deterministic functions, so `pass^k` measures consistency across fault draws, not model sampling variance. No real model is involved.
- Sandbox tests skip where bubblewrap or unprivileged user namespaces are unavailable (including Windows and some hardened CI runners).
