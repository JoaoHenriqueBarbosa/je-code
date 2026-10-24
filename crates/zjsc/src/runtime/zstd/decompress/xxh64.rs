//! XXH64 incremental (semente 0), o checksum de conteúdo do quadro zstd (`lib/common/xxhash.h`).

const P1: u64 = 11400714785074694791;
const P2: u64 = 14029467366897019727;
const P3: u64 = 1609587929392839161;
const P4: u64 = 9650029242287828579;
const P5: u64 = 2870177450012600261;

fn round(acc: u64, input: u64) -> u64 {
    acc.wrapping_add(input.wrapping_mul(P2)).rotate_left(31).wrapping_mul(P1)
}

fn merge_round(acc: u64, val: u64) -> u64 {
    (acc ^ round(0, val)).wrapping_mul(P1).wrapping_add(P4)
}

fn le64(b: &[u8]) -> u64 {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

pub struct Xxh64 {
    v: [u64; 4],
    total: u64,
    mem: Vec<u8>,
}

impl Xxh64 {
    pub fn new() -> Self {
        Xxh64 { v: [P1.wrapping_add(P2), P2, 0, 0u64.wrapping_sub(P1)], total: 0, mem: Vec::with_capacity(32) }
    }

    pub fn update(&mut self, mut input: &[u8]) {
        self.total += input.len() as u64;
        if !self.mem.is_empty() {
            let need = 32 - self.mem.len();
            let take = need.min(input.len());
            self.mem.extend_from_slice(&input[..take]);
            input = &input[take..];
            if self.mem.len() < 32 {
                return;
            }
            let block = std::mem::take(&mut self.mem);
            self.consume(&block);
        }
        while input.len() >= 32 {
            self.consume(&input[..32]);
            input = &input[32..];
        }
        self.mem.extend_from_slice(input);
    }

    fn consume(&mut self, block: &[u8]) {
        for i in 0..4 {
            self.v[i] = round(self.v[i], le64(&block[i * 8..]));
        }
    }

    pub fn digest(&self) -> u64 {
        let mut h = if self.total >= 32 {
            let [v1, v2, v3, v4] = self.v;
            let mut h = v1.rotate_left(1).wrapping_add(v2.rotate_left(7)).wrapping_add(v3.rotate_left(12)).wrapping_add(v4.rotate_left(18));
            for v in self.v {
                h = merge_round(h, v);
            }
            h
        } else {
            P5
        };
        h = h.wrapping_add(self.total);
        let mut rest = &self.mem[..];
        while rest.len() >= 8 {
            h ^= round(0, le64(rest));
            h = h.rotate_left(27).wrapping_mul(P1).wrapping_add(P4);
            rest = &rest[8..];
        }
        if rest.len() >= 4 {
            let k = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as u64;
            h ^= k.wrapping_mul(P1);
            h = h.rotate_left(23).wrapping_mul(P2).wrapping_add(P3);
            rest = &rest[4..];
        }
        for &b in rest {
            h ^= (b as u64).wrapping_mul(P5);
            h = h.rotate_left(11).wrapping_mul(P1);
        }
        h ^= h >> 33;
        h = h.wrapping_mul(P2);
        h ^= h >> 29;
        h = h.wrapping_mul(P3);
        h ^ (h >> 32)
    }
}

#[cfg(test)]
mod tests {
    use super::Xxh64;

    fn digest(data: &[u8]) -> u64 {
        let mut h = Xxh64::new();
        h.update(data);
        h.digest()
    }

    // Vetores medidos no bun 1.4.2 (`Bun.hash.xxHash64(data, 0n)`); os quatro primeiros coincidem
    // com os vetores publicados do xxHash.
    const VECTORS: [(&str, u64); 5] = [
        ("", 0xef46db3751d8e999),
        ("a", 0xd24ec4f1a98c6e5b),
        ("abc", 0x44bc2cf5ad770999),
        ("Nobody inspects the spammish repetition", 0xfbcea83c8a378bf1),
        ("0123456789abcdef0123456789abcdef", 0x642a94958e71e6c5),
    ];

    #[test]
    fn known_vectors() {
        for (text, expected) in VECTORS {
            assert_eq!(digest(text.as_bytes()), expected, "{text:?}");
        }
        assert_eq!(digest(&[b'x'; 100]), 0x92f0de5a88a3c094);
        assert_eq!(digest(b"0123456789abcdef0123456789abcdef0123"), 0xc4255ba3d1af5461);
    }

    #[test]
    fn incremental_matches_one_shot_for_any_split() {
        let data = [b'x'; 100];
        for step in [1usize, 3, 7, 31, 32, 33, 64] {
            let mut h = Xxh64::new();
            for part in data.chunks(step) {
                h.update(part);
            }
            assert_eq!(h.digest(), 0x92f0de5a88a3c094, "step {step}");
        }
    }
}
