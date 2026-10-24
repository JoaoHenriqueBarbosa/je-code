//! Pré-divisor de blocos: `ZSTD_splitBlock_byChunks` (zstd_preSplit.c, libzstd 1.5.7), nível 0 das
//! impressões digitais (amostragem de 1 a cada 43 posições, hash de 8 bits), o que a estratégia dfast usa.

const CHUNK_SIZE: usize = 8 << 10;
const THRESHOLD_PENALTY_RATE: u64 = 16;
const THRESHOLD_BASE: u64 = THRESHOLD_PENALTY_RATE - 2;
const THRESHOLD_PENALTY: u64 = 3;
const HASH_TABLE_SIZE: usize = 1 << 10;
/// `splitLevels[ZSTD_dfast]` do `ZSTD_optimalBlockSize` dá o nível 1 do `ZSTD_splitBlock`, ou seja, o
/// nível 0 de `byChunks`: taxa de amostragem e bits do hash.
const SAMPLING_RATE: usize = 43;
const HASH_LOG: u32 = 8;

struct Fingerprint {
    events: [u32; HASH_TABLE_SIZE],
    nb_events: usize,
}

impl Fingerprint {
    fn new() -> Self {
        Fingerprint { events: [0; HASH_TABLE_SIZE], nb_events: 0 }
    }

    /// `recordFingerprint_generic` (`hash2` com `hashLog == 8` é o próprio byte).
    fn record(&mut self, src: &[u8]) {
        self.events.fill(0);
        let limit = src.len() - 2 + 1;
        for n in (0..limit).step_by(SAMPLING_RATE) {
            self.events[usize::from(src[n])] += 1;
        }
        self.nb_events = limit / SAMPLING_RATE;
    }

    fn merge(&mut self, other: &Fingerprint) {
        for (a, b) in self.events.iter_mut().zip(other.events.iter()) {
            *a += *b;
        }
        self.nb_events += other.nb_events;
    }
}

/// `fpDistance`.
fn distance(a: &Fingerprint, b: &Fingerprint) -> u64 {
    (0..(1usize << HASH_LOG))
        .map(|n| {
            let d = i64::from(a.events[n]) * b.nb_events as i64 - i64::from(b.events[n]) * a.nb_events as i64;
            d.unsigned_abs()
        })
        .sum()
}

/// `compareFingerprints`: verdadeiro quando o trecho novo é "diferente demais" do passado.
fn too_different(reference: &Fingerprint, new: &Fingerprint, penalty: u64) -> bool {
    let p50 = reference.nb_events as u64 * new.nb_events as u64;
    let threshold = p50 * (THRESHOLD_BASE + penalty) / THRESHOLD_PENALTY_RATE;
    distance(reference, new) >= threshold
}

/// `ZSTD_splitBlock` para um bloco de 128 KiB: o tamanho do primeiro sub-bloco.
pub fn split_block(block: &[u8]) -> usize {
    let block_size = block.len();
    let mut past = Fingerprint::new();
    let mut new = Fingerprint::new();
    let mut penalty = THRESHOLD_PENALTY;
    past.record(&block[..CHUNK_SIZE]);
    let mut pos = CHUNK_SIZE;
    while pos <= block_size - CHUNK_SIZE {
        new.record(&block[pos..pos + CHUNK_SIZE]);
        if too_different(&past, &new, penalty) {
            return pos;
        }
        past.merge(&new);
        penalty = penalty.saturating_sub(1);
        pos += CHUNK_SIZE;
    }
    block_size
}
