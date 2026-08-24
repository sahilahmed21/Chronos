//! Single seeded PRNG. Integer nanoseconds only. No `f64`.
//!
//! Xoshiro256**. New instance per run from a `u64` seed. No process-global RNG.
//! Spec: `docs/02-architecture.md` § Time, entropy, identity.

pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut state = seed;
        Self {
            s: [
                splitmix64(&mut state),
                splitmix64(&mut state),
                splitmix64(&mut state),
                splitmix64(&mut state),
            ],
        }
    }

    pub fn u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    pub fn delay_ns(&mut self, min: u64, max: u64) -> u64 {
        if min >= max {
            return min;
        }
        let span = max - min;
        match span.checked_add(1) {
            Some(n) => min + self.u64() % n,
            None => min + self.u64() % span,
        }
    }

    pub fn bool(&mut self, p_millionths: u32) -> bool {
        if p_millionths == 0 {
            return false;
        }
        if p_millionths >= 1_000_000 {
            return true;
        }
        (self.u64() % 1_000_000) < u64::from(p_millionths)
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn same_seed_same_first_1000_u64() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(1);
        for _ in 0..1000 {
            assert_eq!(a.u64(), b.u64());
        }
    }

    #[test]
    fn delay_ns_min_equals_max() {
        let mut rng = Rng::new(1);
        assert_eq!(rng.delay_ns(7, 7), 7);
    }

    #[test]
    fn bool_zero_never_true() {
        let mut rng = Rng::new(1);
        for _ in 0..100 {
            assert!(!rng.bool(0));
        }
    }

    #[test]
    fn bool_million_always_true() {
        let mut rng = Rng::new(1);
        for _ in 0..100 {
            assert!(rng.bool(1_000_000));
        }
    }
}
