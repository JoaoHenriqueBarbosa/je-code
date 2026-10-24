//! rapidhash (v1), o hash de string de três programas que diferem só na semente e no que entra como bytes: o
//! `StringHasher` do WTF do WebKit (o motor JavaScript), o do WTF do Blink (o `HashMap` de chaves do Chromium, como as
//! do `localStorage`) e o `Bun.hash.rapidhash` do bun.

/// Os segredos do rapidhash (`rapid_secret`).
const SECRET: [u64; 3] = [0x2d35_8dcc_aa6c_78a5, 0x8bb8_4b93_962e_acc9, 0x4b33_a62e_d433_d4a3];

/// A semente padrão do rapidhash (`RAPID_SEED`): a do `rapidhash()` sem semente, que o WTF do Blink usa, e a que o
/// `Bun.hash.rapidhash` usa quando a página não passa outra.
pub const DEFAULT_SEED: u64 = 0xbdd8_9aa9_8270_4029;
/// A semente do `StringHasher` do WTF do WebKit (o motor JavaScript): zero.
pub const WEBKIT_SEED: u64 = 0;

/// O produto de 128 bits partido nas metades baixa e alta (`rapid_mum`).
fn mum(a: u64, b: u64) -> (u64, u64) {
    let product = u128::from(a) * u128::from(b);
    (product as u64, (product >> 64) as u64)
}

/// `rapid_mix`: o `mum` com as metades combinadas por XOR.
fn mix(a: u64, b: u64) -> u64 {
    let (low, high) = mum(a, b);
    low ^ high
}

/// O rapidhash de `len` bytes lidos por `byte(i)`: quem hasheia texto dobra cada caractere num byte na hora da leitura,
/// sem montar o buffer.
pub fn rapidhash_by(seed: u64, len: usize, byte: impl Fn(usize) -> u8) -> u64 {
    let word = |offset: usize, width: usize| (0..width).fold(0u64, |acc, i| acc | (u64::from(byte(offset + i)) << (8 * i)));
    let read32 = |offset: usize| word(offset, 4);
    let read64 = |offset: usize| word(offset, 8);
    let mut seed = seed ^ mix(seed ^ SECRET[0], SECRET[1]) ^ len as u64;
    let (a, b);
    if len <= 16 {
        if len >= 4 {
            let delta = (len & 24) >> (len >> 3);
            let last = len - 4;
            a = (read32(0) << 32) | read32(last);
            b = (read32(delta) << 32) | read32(last - delta);
        } else if len > 0 {
            a = (u64::from(byte(0)) << 56) | (u64::from(byte(len >> 1)) << 32) | u64::from(byte(len - 1));
            b = 0;
        } else {
            a = 0;
            b = 0;
        }
    } else {
        let mut offset = 0;
        let mut left = len;
        if left > 48 {
            let mut see1 = seed;
            let mut see2 = seed;
            while left >= 48 {
                seed = mix(read64(offset) ^ SECRET[0], read64(offset + 8) ^ seed);
                see1 = mix(read64(offset + 16) ^ SECRET[1], read64(offset + 24) ^ see1);
                see2 = mix(read64(offset + 32) ^ SECRET[2], read64(offset + 40) ^ see2);
                offset += 48;
                left -= 48;
            }
            seed ^= see1 ^ see2;
        }
        if left > 16 {
            seed = mix(read64(offset) ^ SECRET[2], read64(offset + 8) ^ seed ^ SECRET[1]);
            if left > 32 {
                seed = mix(read64(offset + 16) ^ SECRET[2], read64(offset + 24) ^ seed);
            }
        }
        a = read64(len - 16);
        b = read64(len - 8);
    }
    let (a, b) = mum(a ^ SECRET[1], b ^ seed);
    mix(a ^ SECRET[0] ^ len as u64, b ^ SECRET[1])
}

/// O rapidhash de `input`.
pub fn rapidhash(seed: u64, input: &[u8]) -> u64 {
    rapidhash_by(seed, input.len(), |i| input[i])
}

/// O hash de texto dos dois WTF (`StringHasher::computeHashAndMaskTop8Bits`): texto todo em Latin1 entra com um byte
/// por caractere (assim `"ABC"` em 8 e em 16 bits têm o mesmo hash), o resto como as unidades UTF-16 em
/// little-endian; dos 32 bits baixos saem os 8 altos (as flags do `StringImpl`), e o zero vira o bit alto que sobra.
pub fn wtf_string_hash(seed: u64, units: &[u16]) -> u32 {
    let hash = if units.iter().all(|&unit| unit <= 0xFF) {
        rapidhash_by(seed, units.len(), |i| units[i] as u8)
    } else {
        rapidhash_by(seed, units.len() * 2, |i| (units[i / 2] >> (8 * (i % 2))) as u8)
    };
    let masked = (hash as u32) & ((1 << 24) - 1);
    if masked == 0 { 1 << 23 } else { masked }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os vetores do rapidhash v1 do upstream (`abcdefgh` repetido, cada prefixo, semente padrão).
    #[test]
    fn upstream_vectors() {
        let bytes: Vec<u8> = (0..1024).map(|i| b"abcdefgh"[i % 8]).collect();
        let cases: [(usize, u64); 13] = [
            (0, 0x5a6ef77074ebc84b),
            (1, 0xc11328477bc0f5d1),
            (2, 0x5644ac035e40d569),
            (3, 0x347080fbf5fcd81),
            (4, 0x56b66b8dc802bcc),
            (8, 0xb6bf9055973aac7c),
            (16, 0xed56d62eead1e402),
            (32, 0xc19072d767da8ffb),
            (64, 0x89bb40a9928a4f0d),
            (128, 0xe0af7c5e7b6e29fd),
            (256, 0x9a3ed35fbedfa11a),
            (512, 0x4c684b2119ca19fb),
            (1024, 0x4b575f5bf25600d6),
        ];
        for (size, expected) in cases {
            assert_eq!(rapidhash(DEFAULT_SEED, &bytes[..size]), expected, "size={size}");
        }
    }

    /// O texto em 8 e em 16 bits tem o mesmo hash quando é todo Latin1; fora dele entram os dois bytes de cada unidade.
    #[test]
    fn wtf_text_is_latin1_folded() {
        let wide: Vec<u16> = "ação".encode_utf16().collect();
        assert_eq!(wtf_string_hash(DEFAULT_SEED, &wide), (rapidhash(DEFAULT_SEED, &[b'a', 0xE7, 0xE3, b'o']) as u32) & 0xFF_FFFF);
        let han: Vec<u16> = "中".encode_utf16().collect();
        assert_eq!(wtf_string_hash(DEFAULT_SEED, &han), (rapidhash(DEFAULT_SEED, &[0x2D, 0x4E]) as u32) & 0xFF_FFFF);
    }

    /// As três sementes, cada uma com o programa que a usa: a do WebKit (o `StringHasher` do motor JavaScript, cujo
    /// valor fixa a ordem das propriedades nas tabelas do JSC), a do Blink (a ordem das chaves do `localStorage`,
    /// medida no oráculo) e a padrão do `Bun.hash.rapidhash`.
    #[test]
    fn seeds_per_program() {
        let key: Vec<u16> = "k7".encode_utf16().collect();
        assert_eq!(wtf_string_hash(WEBKIT_SEED, &key), (rapidhash(0, b"k7") as u32) & 0xFF_FFFF);
        // O Blink: no localStorage do oráculo, com oito chaves `k0`..`k7`, `k7` cai no primeiro slot da tabela de 16.
        let slots: Vec<u32> = (0..8).map(|i| wtf_string_hash(DEFAULT_SEED, &format!("k{i}").encode_utf16().collect::<Vec<_>>()) & 15).collect();
        assert!(slots[7] <= *slots.iter().min().unwrap());
        assert_eq!(rapidhash(DEFAULT_SEED, b""), 0x5a6ef77074ebc84b);
    }
}
