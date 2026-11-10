from chronos_agent.ddmin import ddmin
from chronos_agent.rng import Rng
from chronos_agent.report import pass_hat_k, wilson


def test_rng_is_stable_and_seeded():
    a, b = Rng(7), Rng(7)
    assert [a.next() for _ in range(5)] == [b.next() for _ in range(5)]
    assert Rng(7).next() != Rng(8).next()
    assert Rng(7).next() == 7191089600892374487  # pinned: splitmix64 must not drift


def test_ddmin_finds_one_minimal_pair():
    items = list(range(20))
    fails = lambda xs: 3 in xs and 11 in xs
    assert sorted(ddmin(items, fails)) == [3, 11]


def test_ddmin_single_and_passing_inputs():
    assert ddmin([1, 2, 3], lambda xs: 2 in xs) == [2]
    try:
        ddmin([1, 2], lambda xs: False)
        assert False
    except ValueError:
        pass


def test_pass_hat_k_matches_closed_form():
    assert pass_hat_k(10, 10, 5) == 1.0
    assert pass_hat_k(0, 10, 1) == 0.0
    assert abs(pass_hat_k(5, 10, 1) - 0.5) < 1e-12
    assert abs(pass_hat_k(9, 10, 2) - (9 * 8) / (10 * 9)) < 1e-12
    assert pass_hat_k(3, 10, 5) == 0.0  # fewer successes than k


def test_wilson_interval_brackets_estimate():
    lo, hi = wilson(50, 100)
    assert lo < 0.5 < hi and 0 <= lo and hi <= 1
    assert wilson(0, 0) == (0.0, 1.0)
