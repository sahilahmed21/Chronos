import argparse
import json
import sys

from . import agents
from .report import pass_hat_k, wilson
from .shrink import emit_regression, shrink
from .world import run


def detection_table(seeds, profile, start=1):
    seed_range = range(start, start + seeds)
    control = [s for s in seed_range if run(agents.good, s, profile).violations]
    planted = {}
    for name, agent in agents.PLANTED.items():
        hits = [s for s in seed_range if run(agent, s, profile).violations]
        planted[name] = {"first_seed": hits[0] if hits else None, "detections": len(hits)}
    return {"profile": profile, "control_seeds": seeds, "control_false_positives": len(control),
            "planted": planted}


def evaluate(agent, profile, seeds, start=1):
    c = sum(run(agent, s, profile).success for s in range(start, start + seeds))
    return {"profile": profile, "n": seeds, "successes": c, "pass1": c / seeds,
            "pass_hat_5": pass_hat_k(c, seeds, 5), "ci95": wilson(c, seeds)}


def demo():
    agent = agents.PLANTED["retry_no_idem"]
    seed = next(x for x in range(1, 100_000)
                if run(agent, x, "brutal").violations[:1]
                and run(agent, x, "brutal").violations[0].name == "duplicate_effect")
    r = run(agent, seed, "brutal")
    print(f"seed {seed} (brutal): {len(r.faults)} faults injected, {len(r.trace)} tool calls")
    print(f"violation: {r.violations[0].name} - {r.violations[0].detail}")
    res = shrink(agent, seed, "brutal")
    print(f"shrunk {len(r.faults)} -> {len(res.faults)} fault(s): "
          f"{[(f.call, f.kind) for f in res.faults]}")
    a, b = run(agent, seed, "brutal", faults=res.faults), run(agent, seed, "brutal", faults=res.faults)
    print(f"replay digest {a.digest[:16]} == {b.digest[:16]}: {a.digest == b.digest}")
    print("generated regression test:")
    print(emit_regression("retry_no_idem", res))
    good = run(agents.good, seed, "brutal", faults=res.faults)
    print("good agent: no violations" if not good.violations else f"good agent FAILED: {good.violations}")
    return 0 if a.digest == b.digest and not good.violations else 1


def _agent(name):
    if name == "good":
        return agents.good
    if name in agents.PLANTED:
        return agents.PLANTED[name]
    raise SystemExit(f"unknown agent {name!r}; choose good or one of {sorted(agents.PLANTED)}")


def main(argv=None):
    p = argparse.ArgumentParser(prog="chronos_agent")
    sub = p.add_subparsers(dest="cmd", required=True)
    d = sub.add_parser("detect")
    d.add_argument("--seeds", type=int, default=500)
    d.add_argument("--profile", default="brutal")
    e = sub.add_parser("eval")
    e.add_argument("--agent", default="good")
    e.add_argument("--seeds", type=int, default=200)
    s = sub.add_parser("shrink")
    s.add_argument("--agent", required=True)
    s.add_argument("--profile", default="brutal")
    s.add_argument("--seed", type=int)
    s.add_argument("--emit-test", action="store_true")
    sub.add_parser("demo")
    a = p.parse_args(argv)

    if a.cmd == "detect":
        t = detection_table(a.seeds, a.profile)
        print(json.dumps(t, indent=2))
        undetected = [n for n, r in t["planted"].items() if r["first_seed"] is None]
        return 1 if t["control_false_positives"] or undetected else 0
    if a.cmd == "demo":
        return demo()
    if a.cmd == "eval":
        out = [evaluate(_agent(a.agent), pr, a.seeds) for pr in ("none", "mild", "brutal")]
        print(json.dumps(out, indent=2))
        return 0
    agent = _agent(a.agent)
    seed = a.seed
    if seed is None:
        seed = next((x for x in range(1, 100_000) if run(agent, x, a.profile).violations), None)
        if seed is None:
            print("no failing seed found", file=sys.stderr)
            return 1
    res = shrink(agent, seed, a.profile)
    if a.emit_test:
        print(emit_regression(a.agent, res))
    else:
        print(json.dumps({"seed": res.seed, "violation": res.violation,
                          "faults": [[f.call, f.kind] for f in res.faults],
                          "steps_before": res.steps_before, "steps_after": res.steps_after}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
