/// Fast deterministic xorshift64 RNG. Used everywhere: asteroids, particles, effects, nebula, starfield.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 16) as u32
    }

    /// Returns value in [0, 1).
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() & 0x00FF_FFFF) as f32 / 16777216.0
    }

    /// Returns value in [-1, 1).
    pub fn next_signed(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }
}
