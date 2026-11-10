import json

import httpx
import pytest

from chronos_agent.recorder import Recorder, ReplayMiss, attribute, load


def upstream(calls):
    def handler(req: httpx.Request):
        calls.append(req)
        body = {"n": len(calls), "usage": {"input_tokens": 100, "output_tokens": 20}}
        return httpx.Response(200, json=body, headers={"content-type": "application/json"})
    return httpx.MockTransport(handler)


def post(client, text, step=None):
    h = {"authorization": "Bearer sk-secret", "x-api-key": "sk-secret2"}
    if step:
        h["x-chronos-step"] = step
    return client.post("https://api.example/v1/messages", json={"q": text}, headers=h)


def test_record_then_replay_makes_zero_upstream_calls(tmp_path):
    path, calls = tmp_path / "run.jsonl", []
    with httpx.Client(transport=Recorder(path, "record", upstream(calls))) as c:
        first = [post(c, "a").json(), post(c, "b").json(), post(c, "a").json()]
    assert len(calls) == 3
    with httpx.Client(transport=Recorder(path, "replay")) as c:
        again = [post(c, "a").json(), post(c, "b").json(), post(c, "a").json()]
    assert again == first and len(calls) == 3  # identical-request repeats served in order


def test_replay_of_unrecorded_request_fails_loudly(tmp_path):
    path = tmp_path / "run.jsonl"
    with httpx.Client(transport=Recorder(path, "record", upstream([]))) as c:
        post(c, "a")
    with httpx.Client(transport=Recorder(path, "replay")) as c:
        with pytest.raises(ReplayMiss):
            post(c, "different")
        post(c, "a")
        with pytest.raises(ReplayMiss):  # already consumed
            post(c, "a")


def test_secrets_are_never_written_to_disk(tmp_path):
    path = tmp_path / "run.jsonl"
    with httpx.Client(transport=Recorder(path, "record", upstream([]))) as c:
        post(c, "a")
    raw = path.read_text()
    assert "sk-secret" not in raw and "authorization" not in raw.lower()


def test_digest_is_stable_across_20_replays(tmp_path):
    path = tmp_path / "run.jsonl"
    with httpx.Client(transport=Recorder(path, "record", upstream([]))) as c:
        post(c, "a"); post(c, "b")
    digests = set()
    for _ in range(20):
        rec = Recorder(path, "replay")
        with httpx.Client(transport=rec) as c:
            post(c, "a"); post(c, "b")
        digests.add(rec.digest())
    assert len(digests) == 1


def test_attribution_sums_tokens_latency_and_cost_by_step(tmp_path):
    ticks = iter(x / 1000 for x in range(0, 100, 7))  # fake perf_counter: 7 ms per call
    path, calls = tmp_path / "run.jsonl", []
    rec = Recorder(path, "record", upstream(calls), clock=lambda: next(ticks))
    with httpx.Client(transport=rec) as c:
        post(c, "a", step="plan"); post(c, "b", step="plan"); post(c, "c", step="summarize")
    rows = attribute(load(path), usd_per_mtok_in=3.0, usd_per_mtok_out=15.0)
    assert rows["plan"]["calls"] == 2 and rows["plan"]["input_tokens"] == 200
    assert rows["summarize"]["calls"] == 1 and rows["plan"]["latency_ms"] == pytest.approx(14.0)
    assert rows["plan"]["usd"] == pytest.approx(200 * 3e-6 + 40 * 15e-6)
    assert json.loads(path.read_text().splitlines()[0])["step"] == "plan"
