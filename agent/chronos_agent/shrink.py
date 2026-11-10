from dataclasses import dataclass

from .ddmin import ddmin
from .world import run


@dataclass
class ShrinkResult:
    seed: int
    profile: str
    violation: str
    faults: list
    steps_before: int
    steps_after: int


def shrink(agent, seed, profile):
    full = run(agent, seed, profile)
    if not full.violations:
        raise ValueError("run does not fail")
    name = full.violations[0].name

    def fails(fs):
        return any(v.name == name for v in run(agent, seed, profile, faults=fs).violations)

    faults = ddmin(full.faults, fails)
    after = run(agent, seed, profile, faults=faults)
    return ShrinkResult(seed, profile, name, faults, len(full.trace), len(after.trace))


def emit_regression(agent_name, res):
    faults = ", ".join(f"Fault({f.call}, {f.kind!r})" for f in res.faults)
    return (
        "from chronos_agent import agents\n"
        "from chronos_agent.world import Fault, run\n\n"
        f"FAULTS = [{faults}]  # shrunk from seed {res.seed} ({res.profile}): "
        f"{res.steps_before} -> {res.steps_after} calls\n\n\n"
        f"def test_{agent_name}_reproduces_{res.violation}():\n"
        f"    r = run(agents.PLANTED[{agent_name!r}], {res.seed}, {res.profile!r}, faults=FAULTS)\n"
        f"    assert {res.violation!r} in [v.name for v in r.violations]\n\n\n"
        "def test_good_agent_survives_same_faults():\n"
        f"    assert run(agents.good, {res.seed}, {res.profile!r}, faults=FAULTS).violations == []\n"
    )
