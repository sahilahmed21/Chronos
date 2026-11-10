from chronos_agent.world import Fault, World, run, gen_faults
from chronos_agent import agents


def w(*faults, **kw):
    return World(list(faults), **kw)


def test_idempotent_charge_applies_once_even_with_duplicate_delivery():
    world = w(Fault(0, "duplicate"))
    world.call("charge", {"amount": 100, "idem": "k"}, intent="refund")
    assert world.effect_count("charge", "refund") == 1
    assert world.violations == []


def test_duplicate_delivery_without_key_is_a_violation():
    world = w(Fault(0, "duplicate"))
    world.call("charge", {"amount": 100}, intent="refund")
    assert [v.name for v in world.violations] == ["duplicate_effect"]


def test_timeout_after_commit_applies_effect_and_errors():
    world = w(Fault(0, "timeout_after_commit"))
    r = world.call("charge", {"amount": 100}, intent="refund")
    assert (r.ok, r.error) == (False, "timeout") and world.effect_count("charge", "refund") == 1


def test_timeout_before_commit_has_no_effect():
    world = w(Fault(0, "timeout_before_commit"))
    world.call("charge", {"amount": 100}, intent="refund")
    assert world.effect_count("charge", "refund") == 0


def test_outage_fails_a_window_of_calls():
    world = w(Fault(1, "outage"))
    errs = [world.call("read_notes", {}, intent=f"n{i}").error for i in range(10)]
    assert errs[0] == "" and errs[1:9] == ["unavailable"] * 8 and errs[9] == ""


def test_scope_budget_taint_and_step_invariants():
    world = w(scopes={"read_notes"})
    world.call("charge", {"amount": 1}, intent="x")
    assert world.violations[0].name == "scope" and world.effect_count("charge", "x") == 0
    world = w(budget_cents=50)
    world.call("charge", {"amount": 100, "idem": "a"}, intent="a")
    assert "budget" in [v.name for v in world.violations]
    world = w()
    world.call("charge", {"amount": 1, "idem": "t"}, intent="t", provenance="tool")
    assert "tainted_privileged_call" in [v.name for v in world.violations]
    world = w(max_calls=2)
    for i in range(3):
        world.call("read_notes", {}, intent=f"n{i}")
    assert "step_budget" in [v.name for v in world.violations]


def test_false_success_is_detected_at_finish():
    world = w()
    world.finish(claimed_success=True)
    assert "false_success" in [v.name for v in world.violations]


def test_same_seed_same_digest_different_seed_differs():
    a = run(agents.good, 5, "brutal")
    assert a.digest == run(agents.good, 5, "brutal").digest
    assert a.digest != run(agents.good, 6, "brutal").digest
    assert gen_faults(5, "none") == []


def test_good_agent_has_no_violations_under_any_profile():
    for profile in ("none", "mild", "brutal"):
        bad = [s for s in range(1, 301) if run(agents.good, s, profile).violations]
        assert bad == [], (profile, bad[:5])


def test_each_planted_bug_is_detected_by_some_seed():
    for name, agent in agents.PLANTED.items():
        hits = [s for s in range(1, 2001) if run(agent, s, "brutal").violations]
        assert hits, f"{name} never detected in 2000 brutal seeds"
