from chronos_agent import agents
from chronos_agent.world import run
from chronos_agent.shrink import shrink, emit_regression


def first_failing(agent, profile="brutal"):
    return next(s for s in range(1, 3000) if run(agent, s, profile).violations)


def test_shrink_reduces_faults_and_keeps_failing():
    agent = agents.PLANTED["retry_no_idem"]
    seed = first_failing(agent)
    full = run(agent, seed, "brutal")
    res = shrink(agent, seed, "brutal")
    assert res.violation == full.violations[0].name
    assert len(res.faults) <= len(full.faults) and len(res.faults) >= 0
    assert run(agent, seed, "brutal", faults=res.faults).violations  # still fails
    for i in range(len(res.faults)):  # 1-minimal
        rest = res.faults[:i] + res.faults[i + 1:]
        assert not any(v.name == res.violation for v in run(agent, seed, "brutal", faults=rest).violations)


def test_stale_cache_bug_needs_both_faults():
    from chronos_agent.ddmin import ddmin
    from chronos_agent.world import Fault
    agent = agents.PLANTED["stale_cache"]
    both = [Fault(3, "timeout_after_commit"), Fault(4, "stale_read")]
    assert run(agent, 0, "brutal", faults=both).violations
    for one in both:
        assert not run(agent, 0, "brutal", faults=[one]).violations
    noisy = both + [Fault(7, "rate_limit")]
    assert ddmin(noisy, lambda fs: bool(run(agent, 0, "brutal", faults=fs).violations)) == both


def test_emit_regression_is_valid_python_and_names_the_bug():
    agent = agents.PLANTED["retry_no_idem"]
    seed = first_failing(agent)
    src = emit_regression("retry_no_idem", shrink(agent, seed, "brutal"))
    compile(src, "<gen>", "exec")
    assert "retry_no_idem" in src and "Fault(" in src
