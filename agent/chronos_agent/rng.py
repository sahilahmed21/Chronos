"""splitmix64: stable across Python versions, unlike random.Random's helper methods."""
M = (1 << 64) - 1


class Rng:
    def __init__(self, seed: int):
        self.s = seed & M

    def next(self) -> int:
        self.s = (self.s + 0x9E3779B97F4A7C15) & M
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M
        return z ^ (z >> 31)

    def below(self, n: int) -> int:
        return self.next() % n

    def chance(self, ppm: int) -> bool:
        return self.below(1_000_000) < ppm
