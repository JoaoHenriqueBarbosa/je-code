//! Porte de `src/vs/base/common/hash.ts`.
//!
//! Os hashes são `int32` do JavaScript (`| 0`, `Math.imul`), aqui `i32` com aritmética que dá a
//! volta, e o resultado é bit a bit o do original. `hash` e `do_hash` recebem um
//! [`Dynamic`](crate::dynamic::Dynamic) porque `hash<T>(obj)` do original aceita qualquer valor;
//! funções, `bigint` e símbolos caem no valor padrão (617), `Date`/`RegExp` são objetos sem chaves
//! próprias, e as chaves de um objeto entram ordenadas.
//!
//! Texto é `&[u16]` (unidades UTF-16), como o `charCodeAt` do original.
//!
//! Pendente: `hashAsync` (SHA-1 por `crypto.subtle`, assíncrono, sobre `VSBuffer` de `buffer.ts`,
//! ainda não portado). `StringSHA1` está portado e dá o mesmo resumo para texto.

use crate::assert::assert;
use crate::dynamic::Dynamic;
use crate::strings::{compute_code_point, is_high_surrogate, is_low_surrogate};

/// `ToInt32` do JavaScript.
fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    (x.trunc().rem_euclid(4_294_967_296.0) as u32) as i32
}

/// Valor de hash de um valor qualquer.
pub fn hash(obj: &Dynamic) -> i32 {
    do_hash(obj, 0)
}

pub fn do_hash(obj: &Dynamic, hash_val: i32) -> i32 {
    match obj {
        Dynamic::Null => number_hash(349.0, hash_val),
        Dynamic::Array(array) => array_hash(&array.to_vec(), hash_val),
        Dynamic::Object(_) | Dynamic::RegExp(_) | Dynamic::Date(_) | Dynamic::TypedArray(_) => object_hash(obj, hash_val),
        Dynamic::String(s) => string_hash(s.units(), hash_val),
        Dynamic::Bool(b) => boolean_hash(*b, hash_val),
        Dynamic::Number(n) => number_hash(*n, hash_val),
        Dynamic::Undefined => number_hash(937.0, hash_val),
        Dynamic::BigInt(_) | Dynamic::Function(_) => number_hash(617.0, hash_val),
    }
}

/// `hashVal * 31 + val`, mantido como int32.
pub fn number_hash(val: f64, initial_hash_val: i32) -> i32 {
    to_int32(f64::from(initial_hash_val.wrapping_shl(5)) - f64::from(initial_hash_val) + val)
}

fn boolean_hash(b: bool, initial_hash_val: i32) -> i32 {
    number_hash(if b { 433.0 } else { 863.0 }, initial_hash_val)
}

pub fn string_hash(s: &[u16], hash_val: i32) -> i32 {
    let mut hash_val = number_hash(149_417.0, hash_val);
    for &unit in s {
        hash_val = hash_val.wrapping_shl(5).wrapping_sub(hash_val).wrapping_add(i32::from(unit));
    }
    hash_val
}

/// A string pertence a uma amostra estável de `sample_percentage` por cento? Lança se o percentual
/// não for um inteiro entre 0 e 100.
pub fn is_string_in_sample(value: &[u16], sample_percentage: f64) -> bool {
    assert(
        sample_percentage.fract() == 0.0 && sample_percentage.is_finite() && (0.0..=100.0).contains(&sample_percentage),
        "samplePercentage must be an integer between 0 and 100",
    );
    f64::from((string_hash(value, 0) as u32) % 100) < sample_percentage
}

/// Pré-calcula a transformação afim de [`string_hash`] para uma string fixa e acumuladores int32.
pub struct ConstantStringHash {
    multiplier: i32,
    addend: i32,
}

impl ConstantStringHash {
    pub fn new(value: &[u16]) -> Self {
        let mut multiplier: i32 = 31; // a etapa da semente de `string_hash`
        for _ in value {
            multiplier = multiplier.wrapping_mul(31);
        }
        Self { multiplier, addend: string_hash(value, 0) }
    }

    /// Equivale a `string_hash(value, hash_val)`.
    pub fn apply(&self, hash_val: i32) -> i32 {
        hash_val.wrapping_mul(self.multiplier).wrapping_add(self.addend)
    }
}

/// A semente que `array_hash` mistura antes dos elementos.
pub const ARRAY_HASH_SEED: i32 = 104_579;
/// A semente que `object_hash` mistura antes das entradas.
pub const OBJECT_HASH_SEED: i32 = 181_387;

fn array_hash(arr: &[Dynamic], initial_hash_val: i32) -> i32 {
    let initial = number_hash(f64::from(ARRAY_HASH_SEED), initial_hash_val);
    arr.iter().fold(initial, |hash_val, item| do_hash(item, hash_val))
}

fn object_hash(obj: &Dynamic, initial_hash_val: i32) -> i32 {
    let initial = number_hash(f64::from(OBJECT_HASH_SEED), initial_hash_val);
    let mut entries = obj.own_entries();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries.iter().fold(initial, |hash_val, (key, value)| do_hash(value, string_hash(key.units(), hash_val)))
}

const BLOCK_SIZE: usize = 64; // 512 / 8
const UNICODE_REPLACEMENT: u32 = 0xFFFD;

/// SHA-1 sobre texto UTF-16 (codificado em UTF-8, surrogate isolado vira U+FFFD), sem alocar por
/// bloco.
pub struct StringSha1 {
    h: [u32; 5],
    buff: [u8; BLOCK_SIZE + 3], // +3 para caber qualquer sequência UTF-8
    buff_len: usize,
    total_len: u64,
    leftover_high_surrogate: u16,
    finished: bool,
}

impl Default for StringSha1 {
    fn default() -> Self {
        Self::new()
    }
}

impl StringSha1 {
    pub fn new() -> Self {
        Self {
            h: [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0],
            buff: [0; BLOCK_SIZE + 3],
            buff_len: 0,
            total_len: 0,
            leftover_high_surrogate: 0,
            finished: false,
        }
    }

    pub fn update(&mut self, str_units: &[u16]) {
        let str_len = str_units.len() as i64;
        if str_len == 0 {
            return;
        }
        let mut leftover_high_surrogate = self.leftover_high_surrogate;
        let (mut char_code, mut offset) = if leftover_high_surrogate != 0 {
            let c = leftover_high_surrogate;
            leftover_high_surrogate = 0;
            (c, -1_i64)
        } else {
            (str_units[0], 0_i64)
        };

        loop {
            let mut code_point = u32::from(char_code);
            if is_high_surrogate(char_code) {
                if offset + 1 < str_len {
                    let next_char_code = str_units[(offset + 1) as usize];
                    if is_low_surrogate(next_char_code) {
                        offset += 1;
                        code_point = compute_code_point(char_code, next_char_code);
                    } else {
                        // ilegal: caractere de substituição
                        code_point = UNICODE_REPLACEMENT;
                    }
                } else {
                    // o último caractere é a primeira metade de um par
                    leftover_high_surrogate = char_code;
                    break;
                }
            } else if is_low_surrogate(char_code) {
                code_point = UNICODE_REPLACEMENT;
            }

            self.push(code_point);

            offset += 1;
            if offset < str_len {
                char_code = str_units[offset as usize];
            } else {
                break;
            }
        }
        self.leftover_high_surrogate = leftover_high_surrogate;
    }

    fn push(&mut self, code_point: u32) {
        let mut len = self.buff_len;
        let b = &mut self.buff;
        if code_point < 0x0080 {
            b[len] = code_point as u8;
            len += 1;
        } else if code_point < 0x0800 {
            b[len] = 0b1100_0000 | ((code_point & 0x7C0) >> 6) as u8;
            b[len + 1] = 0b1000_0000 | (code_point & 0x3F) as u8;
            len += 2;
        } else if code_point < 0x10000 {
            b[len] = 0b1110_0000 | ((code_point & 0xF000) >> 12) as u8;
            b[len + 1] = 0b1000_0000 | ((code_point & 0xFC0) >> 6) as u8;
            b[len + 2] = 0b1000_0000 | (code_point & 0x3F) as u8;
            len += 3;
        } else {
            b[len] = 0b1111_0000 | ((code_point & 0x1C0000) >> 18) as u8;
            b[len + 1] = 0b1000_0000 | ((code_point & 0x3F000) >> 12) as u8;
            b[len + 2] = 0b1000_0000 | ((code_point & 0xFC0) >> 6) as u8;
            b[len + 3] = 0b1000_0000 | (code_point & 0x3F) as u8;
            len += 4;
        }
        if len >= BLOCK_SIZE {
            self.buff_len = len;
            self.step();
            len -= BLOCK_SIZE;
            self.total_len += BLOCK_SIZE as u64;
            // guarda os últimos 3 bytes, caso o UTF-8 tenha passado do bloco
            self.buff.copy_within(BLOCK_SIZE..BLOCK_SIZE + 3, 0);
        }
        self.buff_len = len;
    }

    /// O resumo em hexadecimal (40 caracteres). Chamadas seguintes devolvem o mesmo valor.
    pub fn digest(&mut self) -> String {
        if !self.finished {
            self.finished = true;
            if self.leftover_high_surrogate != 0 {
                // ilegal: caractere de substituição
                self.leftover_high_surrogate = 0;
                self.push(UNICODE_REPLACEMENT);
            }
            self.total_len += self.buff_len as u64;
            self.wrap_up();
        }
        self.h.iter().map(|h| format!("{h:08x}")).collect()
    }

    fn wrap_up(&mut self) {
        self.buff[self.buff_len] = 0x80;
        self.buff_len += 1;
        self.buff[self.buff_len..].fill(0);

        if self.buff_len > 56 {
            self.step();
            self.buff.fill(0);
        }

        let ml = 8 * self.total_len;
        self.buff[56..60].copy_from_slice(&((ml >> 32) as u32).to_be_bytes());
        self.buff[60..64].copy_from_slice(&(ml as u32).to_be_bytes());
        self.step();
    }

    fn step(&mut self) {
        let mut w = [0u32; 80];
        for (j, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes(self.buff[j * 4..j * 4 + 4].try_into().expect("4 bytes"));
        }
        for j in 16..80 {
            w[j] = (w[j - 3] ^ w[j - 8] ^ w[j - 14] ^ w[j - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = self.h;
        for (j, word) in w.iter().enumerate() {
            let (f, k) = match j {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6_u32),
            };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        for (h, v) in self.h.iter_mut().zip([a, b, c, d, e]) {
            *h = h.wrapping_add(v);
        }
    }
}
