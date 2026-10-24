//! Keccak-f[1600] e as esponjas SHA-3 e SHAKE (FIPS 202), incrementais.

/// Keccak-f[1600]. As constantes de rodada saem do LFSR do padrão e as rotações da caminhada
/// `(x, y) -> (y, 2x + 3y)`, em vez de digitadas.
fn keccak_f(a: &mut [u64; 25]) {
    let mut lfsr: u8 = 1;
    for _ in 0..24 {
        let mut c = [0u64; 5];
        for x in 0..5 {
            c[x] = a[x] ^ a[x + 5] ^ a[x + 10] ^ a[x + 15] ^ a[x + 20];
        }
        for x in 0..5 {
            let d = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            for y in 0..5 {
                a[x + 5 * y] ^= d;
            }
        }
        let mut b = [0u64; 25];
        b[0] = a[0];
        let (mut x, mut y) = (1usize, 0usize);
        for t in 0..24u32 {
            let (nx, ny) = (y, (2 * x + 3 * y) % 5);
            b[nx + 5 * ny] = a[x + 5 * y].rotate_left(((t + 1) * (t + 2) / 2) % 64);
            x = nx;
            y = ny;
        }
        for y in 0..5 {
            for x in 0..5 {
                a[x + 5 * y] = b[x + 5 * y] ^ (!b[(x + 1) % 5 + 5 * y] & b[(x + 2) % 5 + 5 * y]);
            }
        }
        for j in 0..7 {
            let bit = lfsr & 1 != 0;
            lfsr = if lfsr & 0x80 != 0 { (lfsr << 1) ^ 0x71 } else { lfsr << 1 };
            if bit {
                a[0] ^= 1u64 << ((1u32 << j) - 1);
            }
        }
    }
}

/// Maior taxa (bytes por bloco) entre as instâncias: o SHAKE128, com 168.
const MAX_RATE: usize = 168;

/// Esponja Keccak com o sufixo de domínio do SHA-3 (`0x06`) ou do SHAKE (`0x1f`).
#[derive(Clone)]
pub struct Keccak {
    state: [u64; 25],
    rate: usize,
    buf: [u8; MAX_RATE],
    pos: usize,
    suffix: u8,
}

impl Keccak {
    fn with(rate: usize, suffix: u8) -> Keccak {
        Keccak { state: [0; 25], rate, buf: [0; MAX_RATE], pos: 0, suffix }
    }

    /// SHA-3 com resumo de `out_len` bytes (28, 32, 48 ou 64).
    pub fn sha3(out_len: usize) -> Keccak {
        Keccak::with(200 - 2 * out_len, 0x06)
    }

    /// SHAKE com `security` bytes de segurança (16 para o SHAKE128, 32 para o SHAKE256).
    pub fn shake(security: usize) -> Keccak {
        Keccak::with(200 - 2 * security, 0x1f)
    }

    /// Bytes por bloco absorvido (o `block_size` do `hashlib`).
    pub fn rate(&self) -> usize {
        self.rate
    }

    fn absorb(&mut self) {
        for (i, lane) in self.buf[..self.rate].chunks(8).enumerate() {
            let mut w = [0u8; 8];
            w.copy_from_slice(lane);
            self.state[i] ^= u64::from_le_bytes(w);
        }
        keccak_f(&mut self.state);
        self.pos = 0;
    }

    pub fn update(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            let take = (self.rate - self.pos).min(data.len());
            self.buf[self.pos..self.pos + take].copy_from_slice(&data[..take]);
            self.pos += take;
            data = &data[take..];
            if self.pos == self.rate {
                self.absorb();
            }
        }
    }

    /// Fecha a esponja e espreme `out_len` bytes.
    pub fn finalize(mut self, out_len: usize) -> Vec<u8> {
        self.buf[self.pos..self.rate].fill(0);
        self.buf[self.pos] = self.suffix;
        self.buf[self.rate - 1] |= 0x80;
        self.absorb();
        let mut out = Vec::with_capacity(out_len);
        loop {
            for lane in self.state[..self.rate / 8].iter() {
                out.extend_from_slice(&lane.to_le_bytes());
            }
            if out.len() >= out_len {
                break;
            }
            keccak_f(&mut self.state);
        }
        out.truncate(out_len);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::hex_lower;

    #[test]
    fn sha3_and_shake_vectors() {
        let mut h = Keccak::sha3(32);
        h.update(b"abc");
        assert_eq!(hex_lower(&h.finalize(32)), "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532");
        let h = Keccak::sha3(32);
        assert_eq!(hex_lower(&h.finalize(32)), "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a");
        let h = Keccak::shake(16);
        assert_eq!(hex_lower(&h.finalize(32)), "7f9c2ba4e88f827d616045507605853ed73b8093f6efbc88eb1a6eacfa66ef26");
        let h = Keccak::shake(32);
        assert_eq!(hex_lower(&h.finalize(32)), "46b9dd2b0ba88d13233b3feb743eeb243fcd52ea62b81b82b50c27646ed5762f");
    }

    #[test]
    fn incremental_matches_one_shot_and_squeeze_spans_blocks() {
        let data: Vec<u8> = (0..700u32).map(|i| (i * 3 + 1) as u8).collect();
        let mut whole = Keccak::shake(16);
        whole.update(&data);
        let want = whole.finalize(400);
        for step in [1usize, 5, 167, 168, 169, 699] {
            let mut h = Keccak::shake(16);
            for chunk in data.chunks(step) {
                h.update(chunk);
            }
            assert_eq!(h.finalize(400), want, "passo {step}");
        }
        assert_eq!(want.len(), 400);
    }
}
