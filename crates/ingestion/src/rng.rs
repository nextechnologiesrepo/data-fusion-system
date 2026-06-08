//! A tiny deterministic PRNG (SplitMix64).
//!
//! Synthetic generators must be reproducible for replay, so they never touch
//! the OS RNG. Same seed → same stream, on any platform.

pub struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    pub fn new(seed: u64) -> Self {
        DeterministicRng {
            state: seed.wrapping_add(0x9E37_79B9_7F4A_7C15),
        }
    }

    fn next_u64(&mut self) -> u64 {
        // SplitMix64
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[-mag, mag]`.
    pub fn noise(&mut self, mag: f64) -> f64 {
        (self.unit() * 2.0 - 1.0) * mag
    }
}
