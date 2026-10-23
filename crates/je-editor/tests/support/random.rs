//! Porte de `src/vs/editor/test/common/core/random.ts`: o `Random` com semente (`MersenneTwister`), só com o
//! que os testes usam (`nextIntRange`).

/// `Random.create(seed)`: um Mersenne Twister (MT19937) com a mesma semeadura do original.
pub struct Random {
    mt: [u32; 624],
    index: usize,
}

impl Random {
    pub fn create(seed: u32) -> Self {
        let mut mt = [0u32; 624];
        mt[0] = seed;
        for i in 1..624 {
            let s = mt[i - 1] ^ (mt[i - 1] >> 30);
            mt[i] = (((s & 0xffff_0000) >> 16).wrapping_mul(0x6c07_8965) << 16)
                .wrapping_add((s & 0x0000_ffff).wrapping_mul(0x6c07_8965))
                .wrapping_add(i as u32);
        }
        Self { mt, index: 0 }
    }

    fn next_int(&mut self) -> u32 {
        if self.index == 0 {
            self.generate_numbers();
        }

        let mut y = self.mt[self.index];
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;

        self.index = (self.index + 1) % 624;

        y
    }

    /// `nextIntRange(start, endExclusive)`.
    pub fn next_int_range(&mut self, start: i64, end_exclusive: i64) -> i64 {
        let range = (end_exclusive - start) as f64;
        (f64::from(self.next_int()) / (4_294_967_296.0 / range)).floor() as i64 + start
    }

    fn generate_numbers(&mut self) {
        for i in 0..624 {
            let y = (self.mt[i] & 0x8000_0000) + (self.mt[(i + 1) % 624] & 0x7fff_ffff);
            self.mt[i] = self.mt[(i + 397) % 624] ^ (y >> 1);
            if !y.is_multiple_of(2) {
                self.mt[i] ^= 0x9908_b0df;
            }
        }
    }
}
