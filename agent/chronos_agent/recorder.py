"""Record/replay httpx transport for model-API calls.

Cut-point replay: in replay mode with `inner` and `live={steps}`, requests whose
`x-chronos-step` header is in `live` go to `inner`; everything else is served from the record.
SSE bodies are stored whole, so chunk boundaries and incremental timing are not preserved.


Point an SDK at it with `http_client=httpx.Client(transport=Recorder(...))`.
One JSONL envelope per boundary crossing. Request keys hash method+url+body only,
so auth headers never touch disk. Replay serves identical requests in recorded order
and raises ReplayMiss for anything unrecorded; it never falls through to the network.
"""
import base64
import hashlib
import json
import time

import httpx


class ReplayMiss(Exception):
    pass


def request_key(request: httpx.Request) -> str:
    h = hashlib.sha256()
    h.update(request.method.encode() + b"\0" + str(request.url).encode() + b"\0" + request.content)
    return h.hexdigest()


def load(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


class Recorder(httpx.BaseTransport):
    def __init__(self, path, mode, inner=None, clock=time.perf_counter, live=()):
        if mode not in ("record", "replay"):
            raise ValueError(mode)
        if mode == "record" and inner is None:
            raise ValueError("record mode needs an inner transport")
        self.path, self.mode, self.inner, self.clock = path, mode, inner, clock
        self.live = set(live)
        self.seen = {}
        self.served = []
        if mode == "replay":
            for env in load(path):
                self.seen.setdefault(env["key"], []).append(env)
        else:
            open(path, "w", encoding="utf-8").close()

    def handle_request(self, request: httpx.Request) -> httpx.Response:
        request.read()
        key = request_key(request)
        if (self.mode == "replay" and self.inner is not None
                and request.headers.get("x-chronos-step", "model") in self.live):
            return self.inner.handle_request(request)
        if self.mode == "replay":
            queue = self.seen.get(key)
            if not queue:
                raise ReplayMiss(f"no recorded response for {request.method} {request.url}")
            env = queue.pop(0)
            self.served.append(env)
            return self._response(env)
        t0 = self.clock()
        resp = self.inner.handle_request(request)
        body = resp.read()
        latency_ms = (self.clock() - t0) * 1000
        ctype = resp.headers.get("content-type", "")
        usage = {}
        if "json" in ctype:
            try:
                usage = json.loads(body).get("usage", {}) or {}
            except (ValueError, AttributeError):
                pass
        env = {"key": key, "step": request.headers.get("x-chronos-step", "model"),
               "status": resp.status_code, "content_type": ctype,
               "body_b64": base64.b64encode(body).decode(),
               "latency_ms": latency_ms,
               "input_tokens": usage.get("input_tokens", 0),
               "output_tokens": usage.get("output_tokens", 0)}
        with open(self.path, "a", encoding="utf-8") as f:
            f.write(json.dumps(env, sort_keys=True) + "\n")
        return self._response(env)

    @staticmethod
    def _response(env):
        return httpx.Response(env["status"], content=base64.b64decode(env["body_b64"]),
                              headers={"content-type": env["content_type"]})

    def digest(self) -> str:
        h = hashlib.sha256()
        for env in self.served:
            h.update(env["key"].encode() + hashlib.sha256(base64.b64decode(env["body_b64"])).digest())
        return h.hexdigest()


def attribute(envelopes, usd_per_mtok_in, usd_per_mtok_out):
    """Per-step totals. Prices are caller-supplied: none are hardcoded here."""
    rows = {}
    for e in envelopes:
        r = rows.setdefault(e["step"], {"calls": 0, "input_tokens": 0, "output_tokens": 0,
                                        "latency_ms": 0.0, "usd": 0.0})
        r["calls"] += 1
        r["input_tokens"] += e["input_tokens"]
        r["output_tokens"] += e["output_tokens"]
        r["latency_ms"] += e["latency_ms"]
        r["usd"] += e["input_tokens"] * usd_per_mtok_in / 1e6 + e["output_tokens"] * usd_per_mtok_out / 1e6
    return rows
