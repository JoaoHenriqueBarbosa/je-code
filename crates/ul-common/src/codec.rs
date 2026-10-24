//! Codificações que os programas repetiam: base64, CRC-32 e hexadecimal minúsculo.
//!
//! O base64 aqui é o do `binascii.a2b_base64` do CPython (a decodificação tolera lixo e quebra de
//! linha fora do modo estrito); os chamadores que só codificam não enxergam a diferença.

/// Alfabeto base64 padrão (RFC 4648, seção 4).
pub const BASE64_STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Alfabeto base64 seguro para URL (RFC 4648, seção 5): `-` e `_` no lugar de `+` e `/`.
pub const BASE64_URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Codifica em base64 com o alfabeto dado, com preenchimento `=` se `pad`.
pub fn base64_encode(data: &[u8], alphabet: &[u8; 64], pad: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(alphabet[((n >> 18) & 63) as usize]);
        out.push(alphabet[((n >> 12) & 63) as usize]);
        if chunk.len() > 1 {
            out.push(alphabet[((n >> 6) & 63) as usize]);
        } else if pad {
            out.push(b'=');
        }
        if chunk.len() > 2 {
            out.push(alphabet[(n & 63) as usize]);
        } else if pad {
            out.push(b'=');
        }
    }
    out
}

/// Base64 padrão com preenchimento, como texto (o `Authorization: Basic`, as chaves do yaml).
pub fn base64_string(data: &[u8]) -> String {
    base64_encode(data, BASE64_STANDARD, true).into_iter().map(char::from).collect()
}

fn base64_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodifica base64 (alfabeto padrão) como o `binascii.a2b_base64`. Fora do modo `strict` os
/// caracteres fora do alfabeto são descartados; no modo estrito viram erro. O erro é a mensagem do
/// `binascii.Error` do CPython 3.13.
pub fn base64_decode(data: &[u8], strict: bool) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut quad: usize = 0;
    let mut left: u8 = 0;
    let mut pads: usize = 0;
    let mut padding_started = false;
    for (idx, &c) in data.iter().enumerate() {
        if c == b'=' {
            padding_started = true;
            if quad >= 2 {
                pads += 1;
                if quad + pads >= 4 {
                    if strict && idx + 1 < data.len() {
                        return Err("Excess data after padding".to_string());
                    }
                    quad = 0;
                    break;
                }
            } else if strict && quad == 0 {
                return Err("Leading padding not allowed".to_string());
            }
            continue;
        }
        let Some(v) = base64_value(c) else {
            if strict {
                return Err("Only base64 data is allowed".to_string());
            }
            continue;
        };
        if strict && padding_started {
            return Err("Excess data after padding".to_string());
        }
        pads = 0;
        match quad {
            0 => {
                quad = 1;
                left = v;
            }
            1 => {
                quad = 2;
                out.push((left << 2) | (v >> 4));
                left = v & 0xf;
            }
            2 => {
                quad = 3;
                out.push((left << 4) | (v >> 2));
                left = v & 0x3;
            }
            _ => {
                quad = 0;
                out.push((left << 6) | v);
                left = 0;
            }
        }
    }
    if quad != 0 {
        if quad == 1 {
            return Err(format!(
                "Invalid base64-encoded string: number of data characters ({}) cannot be 1 more than a multiple of 4",
                (out.len() / 3) * 4 + 1
            ));
        }
        return Err("Incorrect padding".to_string());
    }
    Ok(out)
}

/// Hexadecimal em minúsculas, dois dígitos por byte.
pub fn hex_lower(data: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

/// A tabela de um CRC refletido de 32 bits com o polinômio `poly` (0xEDB88320 no CRC-32 de zlib, 0x82F63B78 no
/// CRC-32C de Castagnoli).
const fn build_crc_table(poly: u32) -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { poly ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

const fn build_crc32_table() -> [u32; 256] {
    build_crc_table(0xEDB8_8320)
}

/// A tabela do CRC-32C (polinômio refletido 0x82F63B78), o que o leveldb, o iSCSI e o ext4 usam.
static CRC32C_TABLE: [u32; 256] = build_crc_table(0x82F6_3B78);

/// Continua um CRC-32C a partir de `value` (o CRC já finalizado dos bytes anteriores, 0 no começo).
pub fn crc32c_update(value: u32, data: &[u8]) -> u32 {
    let mut crc = !value;
    for &byte in data {
        crc = CRC32C_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}

/// O CRC-32C de `data`.
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c_update(0, data)
}

/// O CRC "mascarado" do leveldb: guardar o CRC de dados que contêm CRCs deixa de dar o valor trivial. Gira 15 bits
/// e soma uma constante (`crc32c::Mask` do leveldb).
pub fn crc32c_mask(crc: u32) -> u32 {
    ((crc >> 15) | (crc << 17)).wrapping_add(0xa282_ead8)
}

/// Desfaz [`crc32c_mask`].
pub fn crc32c_unmask(masked: u32) -> u32 {
    let rotated = masked.wrapping_sub(0xa282_ead8);
    (rotated >> 17) | (rotated << 15)
}

/// A tabela do CRC-32 (polinômio refletido 0xEDB88320), a de zlib, gzip, zip e PNG.
pub static CRC32_TABLE: [u32; 256] = build_crc32_table();

/// Um passo do CRC-32 sobre o registrador cru (sem a inversão inicial e final): é o `CRC32(c, b)` do
/// `crypt.c` do zip, que a cifra tradicional do PKZIP usa nas chaves.
pub fn crc32_step(crc: u32, byte: u8) -> u32 {
    CRC32_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8)
}

/// As oito tabelas do "slicing-by-8": `[0]` é a `CRC32_TABLE`, e `[k][n]` é o CRC do byte `n` seguido
/// de `k` bytes zero, o que deixa o laço quente consumir oito bytes por passo.
static CRC32_SLICES: [[u32; 256]; 8] = build_crc32_slices();

const fn build_crc32_slices() -> [[u32; 256]; 8] {
    let mut tables = [[0u32; 256]; 8];
    tables[0] = build_crc32_table();
    let mut k = 1;
    while k < 8 {
        let mut n = 0;
        while n < 256 {
            let prev = tables[k - 1][n];
            tables[k][n] = tables[0][(prev & 0xff) as usize] ^ (prev >> 8);
            n += 1;
        }
        k += 1;
    }
    tables
}

/// Continua um CRC-32 a partir de `value` (o CRC já finalizado dos bytes anteriores, 0 no começo).
pub fn crc32_update(value: u32, data: &[u8]) -> u32 {
    let t = &CRC32_SLICES;
    let mut crc = !value;
    let mut chunks = data.chunks_exact(8);
    for c in &mut chunks {
        let lo = u32::from_le_bytes([c[0], c[1], c[2], c[3]]) ^ crc;
        let hi = u32::from_le_bytes([c[4], c[5], c[6], c[7]]);
        crc = t[7][(lo & 0xff) as usize]
            ^ t[6][((lo >> 8) & 0xff) as usize]
            ^ t[5][((lo >> 16) & 0xff) as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[3][(hi & 0xff) as usize]
            ^ t[2][((hi >> 8) & 0xff) as usize]
            ^ t[1][((hi >> 16) & 0xff) as usize]
            ^ t[0][(hi >> 24) as usize];
    }
    for &b in chunks.remainder() {
        crc = crc32_step(crc, b);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encode_vectors() {
        assert_eq!(base64_encode(b"", BASE64_STANDARD, true), b"");
        assert_eq!(base64_encode(b"f", BASE64_STANDARD, true), b"Zg==");
        assert_eq!(base64_encode(b"fo", BASE64_STANDARD, true), b"Zm8=");
        assert_eq!(base64_encode(b"foo", BASE64_STANDARD, true), b"Zm9v");
        assert_eq!(base64_encode(b"foobar", BASE64_STANDARD, true), b"Zm9vYmFy");
        assert_eq!(base64_encode(b"f", BASE64_STANDARD, false), b"Zg");
        assert_eq!(base64_encode(b"\xfb\xff", BASE64_STANDARD, true), b"+/8=");
        assert_eq!(base64_encode(b"\xfb\xff", BASE64_URL, true), b"-_8=");
        assert_eq!(base64_string(b"user:pass"), "dXNlcjpwYXNz");
    }

    #[test]
    fn base64_decode_modes() {
        assert_eq!(base64_decode(b"", false).unwrap(), Vec::<u8>::new());
        assert_eq!(base64_decode(b"Zm9v", true).unwrap(), b"foo".to_vec());
        assert_eq!(base64_decode(b"aGVsbG8=\n", false).unwrap(), b"hello".to_vec());
        // Sem o modo estrito, o que está fora do alfabeto é descartado.
        assert_eq!(base64_decode(b"aGVs\nbG8=", false).unwrap(), b"hello".to_vec());
        assert_eq!(base64_decode(b"Zm9v!", true).unwrap_err(), "Only base64 data is allowed");
        assert_eq!(base64_decode(b"aGVsbG8", false).unwrap_err(), "Incorrect padding");
        assert_eq!(base64_decode(b"=Zm9v", true).unwrap_err(), "Leading padding not allowed");
        assert_eq!(base64_decode(b"Zg==Zg==", true).unwrap_err(), "Excess data after padding");
        assert!(base64_decode(b"Z", false).unwrap_err().starts_with("Invalid base64-encoded string"));
    }

    #[test]
    fn base64_roundtrip_all_lengths() {
        let data: Vec<u8> = (0..=255u8).collect();
        for n in 0..data.len() {
            let enc = base64_encode(&data[..n], BASE64_STANDARD, true);
            assert_eq!(base64_decode(&enc, true).unwrap(), data[..n].to_vec());
        }
    }

    #[test]
    fn hex_lower_digits() {
        assert_eq!(hex_lower(b"\x00\xffAb"), "00ff4162");
        assert_eq!(hex_lower(b""), "");
    }

    #[test]
    fn crc32_known_values() {
        assert_eq!(crc32_update(0, b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32_update(0, b""), 0);
        assert_eq!(crc32_update(0, b"a"), 0xe8b7_be43);
        let (a, b) = b"123456789".split_at(4);
        assert_eq!(crc32_update(crc32_update(0, a), b), 0xcbf4_3926);
    }

    #[test]
    fn crc32_slicing_matches_bytewise() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        for len in [0, 1, 7, 8, 9, 63, 64, 65, 999, 1000] {
            let slow = !data[..len].iter().fold(!0u32, |crc, &b| crc32_step(crc, b));
            assert_eq!(crc32_update(0, &data[..len]), slow, "len {len}");
        }
    }

    #[test]
    fn crc32_table_and_step() {
        assert_eq!(CRC32_TABLE[0], 0);
        assert_eq!(CRC32_TABLE[1], 0x7707_3096);
        assert_eq!(CRC32_TABLE[255], 0x2d02_ef8d);
        // O passo cru, encadeado com as inversões, é o crc32_update.
        assert_eq!(!crc32_step(crc32_step(!0, b'a'), b'b'), crc32_update(0, b"ab"));
    }

    /// Os vetores do RFC 3720 (apêndice B.4) e do `crc32c_test.cc` do leveldb.
    #[test]
    fn crc32c_known_values() {
        assert_eq!(crc32c(b"123456789"), 0xe306_9283);
        assert_eq!(crc32c(&[0u8; 32]), 0x8a91_36aa);
        assert_eq!(crc32c(&[0xffu8; 32]), 0x62a8_ab43);
        let ascending: Vec<u8> = (0..32).collect();
        assert_eq!(crc32c(&ascending), 0x46dd_794e);
        let (a, b) = b"123456789".split_at(4);
        assert_eq!(crc32c_update(crc32c(a), b), 0xe306_9283);
    }

    #[test]
    fn crc32c_mask_round_trips() {
        for crc in [0u32, 1, 0xe306_9283, 0xffff_ffff] {
            assert_eq!(crc32c_unmask(crc32c_mask(crc)), crc);
            assert_ne!(crc32c_mask(crc), crc);
        }
    }
}
