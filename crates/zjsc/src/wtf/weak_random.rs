//! Tradução de `wtf/WeakRandom.h` (xorshift128+, Vigna 2014) e, de `wtf/CryptographicallyRandomNumber.h`, o
//! `cryptographicallyRandomNumber<unsigned>()` que semeia o gerador.
//!
//! DIVERGÊNCIA: `cryptographicallyRandomNumber` lê `arc4random`/`/dev/urandom` no C++. Aqui a semente vem do
//! `random_fill` do `Host` instalado (o `getrandom` do pseudo-processo no sandbox). Os offsets
//! `lowOffset`/`highOffset` e `generate` existem só para o JIT inlinar o gerador e não são portados.

/// `cryptographicallyRandomNumber<unsigned>()`.
pub fn cryptographically_random_number() -> u32 {
    let mut bytes = [0u8; 4];
    crate::runtime::host::host().random_fill(&mut bytes);
    u32::from_le_bytes(bytes)
}

/// `class WeakRandom`.
#[derive(Clone, Copy, Debug)]
pub struct WeakRandom {
    seed: u32,
    low: u64,
    high: u64,
    /// O `Math.random` do V8: o mesmo xorshift128+, mas o `double` sai dos 53 bits altos da soma.
    v8: bool,
}

impl Default for WeakRandom {
    /// `WeakRandom(unsigned seed = cryptographicallyRandomNumber<unsigned>())`.
    fn default() -> WeakRandom {
        WeakRandom::new(cryptographically_random_number())
    }
}

/// `base::RandomNumberGenerator::MurmurHash3` do V8 (o `fmix64` do MurmurHash3).
const fn murmur_hash3(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    h
}

impl WeakRandom {
    /// `WeakRandom(seed)`.
    pub fn new(seed: u32) -> WeakRandom {
        let mut random = WeakRandom { seed: 0, low: 0, high: 0, v8: false };
        random.set_seed(seed);
        random
    }

    /// O estado do `Math.random` do V8 semeado com `seed` (`MathRandom::InitializeAndMaybeRefillCache` com
    /// `--random-seed`): `s0 = MurmurHash3(seed)`, `s1 = MurmurHash3(~seed)`, e a cada chamada um passo e
    /// `((s0 + s1) >> 11) * 2^-53` (conferido contra o Chromium 154 com `--random-seed=1`).
    pub fn v8_seeded(seed: u64) -> WeakRandom {
        WeakRandom { seed: seed as u32, low: murmur_hash3(seed), high: murmur_hash3(!seed), v8: true }
    }

    /// `setSeed(seed)`.
    pub fn set_seed(&mut self, seed: u32) {
        self.seed = seed;

        // A zero seed would cause an infinite series of zeroes.
        let seed = if seed == 0 { 1 } else { seed };

        self.low = seed as u64;
        self.high = seed as u64;
        self.advance();
    }

    /// `seed()`.
    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// `get()`: um `double` em [0, 1) com 53 bits.
    pub fn get(&mut self) -> f64 {
        let sum = self.advance();
        let value = if self.v8 { sum >> 11 } else { sum & ((1u64 << 53) - 1) };
        value as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// `getUint32()`.
    pub fn get_uint32(&mut self) -> u32 {
        self.advance() as u32
    }

    /// `getUint32(limit)`.
    pub fn get_uint32_below(&mut self, limit: u32) -> u32 {
        if limit <= 1 {
            return 0;
        }
        let cutoff = (u32::MAX as u64 + 1) / limit as u64 * limit as u64;
        loop {
            let value = self.get_uint32() as u64;
            if value >= cutoff {
                continue;
            }
            return (value % limit as u64) as u32;
        }
    }

    /// `getUint64()`.
    pub fn get_uint64(&mut self) -> u64 {
        self.advance()
    }

    /// `returnTrueWithProbability(probability)`.
    pub fn return_true_with_probability(&mut self, probability: f64) -> bool {
        debug_assert!((0.0..=1.0).contains(&probability));

        if probability == 0.0 {
            return false;
        }

        let value = self.get_uint32() as f64;
        value <= u32::MAX as f64 * probability
    }

    /// `nextState(x, y)`.
    pub const fn next_state(mut x: u64, y: u64) -> u64 {
        x ^= x << 23;
        x ^= x >> 17;
        x ^= y ^ (y >> 26);
        x
    }

    fn advance(&mut self) -> u64 {
        let x = self.low;
        let y = self.high;
        self.low = y;
        self.high = WeakRandom::next_state(x, y);
        self.high.wrapping_add(self.low)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os seis primeiros `Math.random()` de uma página no Chromium 154 com `--js-flags=--random-seed=1`.
    #[test]
    fn v8_seed_reproduces_chromium() {
        let mut random = WeakRandom::v8_seeded(1);
        let got: Vec<f64> = (0..6).map(|_| random.get()).collect();
        assert_eq!(
            got,
            [0.16917030546536072, 0.9971327925079954, 0.4401605565829476, 0.3133269805487947, 0.27066156786823625, 0.22758598914788974]
        );
    }

    #[test]
    fn same_seed_same_sequence() {
        let mut a = WeakRandom::new(42);
        let mut b = WeakRandom::new(42);
        for _ in 0..16 {
            assert_eq!(a.get_uint64(), b.get_uint64());
        }
    }

    #[test]
    fn zero_seed_is_not_degenerate() {
        let mut random = WeakRandom::new(0);
        assert_eq!(random.seed(), 0);
        assert_ne!(random.get_uint64(), 0);
    }

    #[test]
    fn get_is_in_unit_interval() {
        let mut random = WeakRandom::new(7);
        for _ in 0..1000 {
            let value = random.get();
            assert!((0.0..1.0).contains(&value));
        }
    }

    #[test]
    fn bounded_values_stay_below_limit() {
        let mut random = WeakRandom::new(99);
        for _ in 0..1000 {
            assert!(random.get_uint32_below(10) < 10);
        }
        assert_eq!(random.get_uint32_below(1), 0);
    }
}
