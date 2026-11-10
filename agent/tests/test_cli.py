import json

from chronos_agent import agents
from chronos_agent.cli import detection_table, evaluate, main


def test_detection_table_flags_every_planted_bug_and_clears_the_control():
    t = detection_table(seeds=500, profile="brutal")
    assert t["control_false_positives"] == 0 and t["control_seeds"] == 500
    for name in agents.PLANTED:
        assert t["planted"][name]["first_seed"] is not None, name
        assert t["planted"][name]["detections"] >= 1


def test_evaluate_reports_consistency_with_interval():
    e = evaluate(agents.good, "none", seeds=50)
    assert e["pass1"] == 1.0 and e["pass_hat_5"] == 1.0 and e["n"] == 50
    lo, hi = e["ci95"]
    assert lo > 0.9 and hi == 1.0
    b = evaluate(agents.good, "brutal", seeds=200)
    assert 0 < b["pass1"] < 1 and b["pass_hat_5"] < b["pass1"]


def test_cli_shrink_prints_json(capsys):
    assert main(["shrink", "--agent", "retry_no_idem", "--profile", "brutal"]) == 0
    out = json.loads(capsys.readouterr().out)
    assert out["violation"] and out["steps_after"] <= out["steps_before"]


def test_cli_demo_tells_the_whole_story(capsys):
    assert main(["demo"]) == 0
    out = capsys.readouterr().out
    for needle in ("seed", "violation: duplicate_effect", "shrunk", "replay digest", "good agent: no violations"):
        assert needle in out, needle


def test_cli_detect_succeeds_when_all_bugs_found_and_control_clean(capsys):
    assert main(["detect", "--seeds", "500"]) == 0
    assert "retry_no_idem" in capsys.readouterr().out
