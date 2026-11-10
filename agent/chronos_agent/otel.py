"""Export recorded model-call envelopes as OpenTelemetry-style GenAI spans (plain dicts).

SEMCONV_NOTE: the `gen_ai.*` attributes are at "Development" stability in the OpenTelemetry
spec (checked 2026-11); names here may change. Ids are derived from content so output is
deterministic. This emits data only; it does not depend on the OpenTelemetry SDK.
"""
import hashlib

SEMCONV_NOTE = "gen_ai.* attributes are Development-stability (OpenTelemetry, checked 2026-11)"


def _id(*parts, nbytes):
    return hashlib.sha256("|".join(parts).encode()).hexdigest()[: nbytes * 2]


def to_spans(envelopes, trace_seed):
    trace_id = _id(trace_seed, nbytes=16)
    spans, t = [], 0
    for i, e in enumerate(envelopes):
        dur = max(int(e["latency_ms"] * 1_000_000), 1)
        spans.append({
            "trace_id": trace_id,
            "span_id": _id(trace_seed, str(i), e["key"], nbytes=8),
            "name": "chat",
            "start_ns": t,
            "end_ns": t + dur,
            "attributes": {
                "gen_ai.operation.name": "chat",
                "gen_ai.usage.input_tokens": e["input_tokens"],
                "gen_ai.usage.output_tokens": e["output_tokens"],
                "http.response.status_code": e["status"],
                "chronos.step": e["step"],
                "chronos.envelope_key": e["key"],
            },
        })
        t += dur
    return spans
