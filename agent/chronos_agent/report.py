from math import comb, sqrt


def pass_hat_k(c: int, n: int, k: int) -> float:
    """Probability all k trials drawn from n (c successes) succeed: C(c,k)/C(n,k)."""
    return comb(c, k) / comb(n, k) if c >= k else 0.0


def wilson(c: int, n: int, z: float = 1.96):
    if n == 0:
        return (0.0, 1.0)
    p = c / n
    d = 1 + z * z / n
    mid = (p + z * z / (2 * n)) / d
    half = z * sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, mid - half), min(1.0, mid + half))
