import json

import httpx
import pytest

from chronos_agent import otel
from chronos_agent.llm_agent import llm_agent
from chronos_agent.recorder import Recorder, ReplayMiss, load
from chronos_agent.world import Fault, run


def scripted(steps, calls=None):
    """Mock model API replaying a fixed list of tool_use turns."""
    it = iter(steps)

    def handler(req: httpx.Request):
        if calls is not None:
            calls.append(json.loads(req.content))
        name, args = next(it)
        body = {"stop_reason": "tool_use", "usage": {"input_tokens": 50, "output_tokens": 10},
                "content": [{"type": "tool_use", "id": f"tu{len(calls or [])}", "name": name, "input": args}]}
        return httpx.Response(200, json=body)
    return httpx.MockTransport(handler)


HAPPY = [("read_notes", {}), ("charge", {"amount": 2000, "idem": "refund"}),
         ("create_ticket", {"title": "refund-7", "idem": "ticket"}), ("finish", {"success": True})]


def agent_for(transport, **kw):
    return llm_agent(httpx.Client(transport=transport), model="test-model", api_key="sk-test-key", **kw)


def test_llm_agent_completes_task_through_the_world():
    r = run(agent_for(scripted(HAPPY)), 0, "none")
    assert r.claimed and r.success and r.violations == []


def test_llm_agent_sends_tools_and_keeps_the_api_key_out_of_the_body():
    calls = []
    run(agent_for(scripted(HAPPY, calls)), 0, "none")
    assert {t["name"] for t in calls[0]["tools"]} >= {"charge", "create_ticket", "find_ticket", "read_notes", "finish"}
    assert "sk-test-key" not in json.dumps(calls)
    assert calls[-1]["messages"][-1]["content"][0]["type"] == "tool_result"


def test_model_that_obeys_injected_notes_trips_the_taint_invariant():
    steps = [("read_notes", {}), ("charge", {"amount": 2000, "idem": "refund"}),
             ("charge", {"amount": 500, "idem": "inj"}),
             ("create_ticket", {"title": "refund-7", "idem": "ticket"}), ("finish", {"success": True})]
    r = run(agent_for(scripted(steps)), 0, "none")
    assert "tainted_privileged_call" in [v.name for v in r.violations]


def test_model_that_retries_with_idem_key_survives_timeout_after_commit():
    steps = [("read_notes", {}), ("charge", {"amount": 2000, "idem": "refund"}),
             ("charge", {"amount": 2000, "idem": "refund"}),
             ("create_ticket", {"title": "refund-7", "idem": "ticket"}), ("finish", {"success": True})]
    r = run(agent_for(scripted(steps)), 0, "none", faults=[Fault(1, "timeout_after_commit")])
    assert r.violations == [] and r.success


def test_max_turns_stops_a_model_that_never_finishes():
    r = run(agent_for(scripted([("read_notes", {})] * 20), max_turns=3), 0, "none")
    assert not r.claimed


def test_recorded_agent_run_replays_with_zero_upstream_calls(tmp_path):
    path, calls = tmp_path / "run.jsonl", []
    first = run(agent_for(Recorder(path, "record", scripted(HAPPY, calls))), 0, "none")
    n_live = len(calls)
    digests = set()
    for _ in range(3):
        rp = Recorder(path, "replay")
        again = run(agent_for(rp), 0, "none")
        digests.add((again.digest, rp.digest()))
        assert again.success and again.digest == first.digest
    assert len(digests) == 1 and len(calls) == n_live
    assert "sk-test-key" not in path.read_text()


def test_cut_point_replay_runs_chosen_steps_live(tmp_path):
    path = tmp_path / "run.jsonl"
    run(agent_for(Recorder(path, "record", scripted(HAPPY, []))), 0, "none")
    assert [e["step"] for e in load(path)] == ["turn-0", "turn-1", "turn-2", "turn-3"]
    live_calls = []
    live = scripted([("finish", {"success": False})], live_calls)
    cut = Recorder(path, "replay", inner=live, live={"turn-3"})
    r = run(agent_for(cut), 0, "none")
    assert len(live_calls) == 1 and not r.claimed  # only turn-3 went live, and it changed the outcome
    strict = Recorder(path, "replay", inner=live, live=set())
    with pytest.raises(ReplayMiss):
        strict.handle_request(httpx.Request("POST", "https://x/other", content=b"{}"))


def test_sse_responses_round_trip(tmp_path):
    sse = httpx.MockTransport(lambda r: httpx.Response(
        200, content=b"data: a\n\ndata: b\n\n", headers={"content-type": "text/event-stream"}))
    path = tmp_path / "r.jsonl"
    with httpx.Client(transport=Recorder(path, "record", sse)) as c:
        live = c.post("https://api/x", json={"q": 1}).text
    with httpx.Client(transport=Recorder(path, "replay")) as c:
        assert c.post("https://api/x", json={"q": 1}).text == live == "data: a\n\ndata: b\n\n"


def test_otel_spans_are_deterministic_and_carry_usage(tmp_path):
    path = tmp_path / "run.jsonl"
    ticks = iter(x / 100 for x in range(0, 100, 5))
    run(agent_for(Recorder(path, "record", scripted(HAPPY, []), clock=lambda: next(ticks))), 0, "none")
    env = load(path)
    a, b = otel.to_spans(env, trace_seed="s"), otel.to_spans(env, trace_seed="s")
    assert a == b and len(a) == 4
    s = a[0]
    assert s["attributes"]["gen_ai.usage.input_tokens"] == 50
    assert s["attributes"]["chronos.step"] == "turn-0"
    assert s["attributes"]["gen_ai.operation.name"] == "chat" and s["end_ns"] > s["start_ns"]
    assert otel.SEMCONV_NOTE
