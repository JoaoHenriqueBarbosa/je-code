//! Tradução de `WTF/wtf/text/StringHasher.h`, `StringHasherInlines.h` e do `RapidHash.h` que o
//! `StringHasher` usa de fato (o `SuperFastHash.h` não participa do hash do `StringImpl`).
//!
//! O hash de string do WebKit é o rapidhash com semente zero (o núcleo está no
//! `ul_common::hash::rapidhash`, que o WTF do Blink e o `Bun.hash` também usam): se todos os caracteres
//! de um buffer de 16 bits são Latin1, cada um vira um byte (assim `"ABC"` Latin1 e `u"ABC"` têm o
//! mesmo hash); caso contrário os `2N` bytes little-endian entram como estão.

use crate::wtf::rapidhash::{rapidhash_by, WEBKIT_SEED};

use crate::wtf::text::string_impl::CharType;

/// Razão áurea. Valor inicial arbitrário, para não mapear zeros em hash zero.
pub const STRING_HASHING_START_VALUE: u32 = 0x9E37_79B9;

/// Oito bits do hash são reservados para as flags do `StringImpl`.
pub const FLAG_COUNT: u32 = 8;
pub const MASK_HASH: u32 = (1u32 << (u32::BITS - FLAG_COUNT)) - 1;

/// Equivalente do parâmetro de template `Converter` do C++: transforma o caractere antes de entrar
/// no hash. `IS_DEFAULT` marca o `DefaultConverter`, que dispara o caminho Latin1/bytes crus.
pub trait Converter {
    const IS_DEFAULT: bool;
    fn convert(character: u16) -> u16;
}

/// `StringHasher::DefaultConverter`: o próprio caractere.
pub struct DefaultConverter;

impl Converter for DefaultConverter {
    const IS_DEFAULT: bool = true;
    fn convert(character: u16) -> u16 {
        character
    }
}

/// Evita devolver hash 0, que sinaliza "hash ainda não calculado". O bit alto preserva boa
/// fidelidade a um hash 0, que provavelmente daria 0 depois que a busca mascara os bits altos.
pub const fn avoid_zero(hash: u32) -> u32 {
    if hash != 0 {
        return hash;
    }
    0x8000_0000u32 >> FLAG_COUNT
}

pub const fn avalanche_bits(hash: u32) -> u32 {
    let mut result = hash;

    result ^= result << 3;
    result = result.wrapping_add(result >> 5);
    result ^= result << 2;
    result = result.wrapping_add(result >> 15);
    result ^= result << 10;

    result
}

pub const fn finalize(hash: u32) -> u32 {
    avoid_zero(avalanche_bits(hash))
}

pub const fn finalize_and_mask_top8_bits(hash: u32) -> u32 {
    // Reservar os bits altos para flags preserva a maior parte do valor do hash, já que a busca
    // costuma mascarar esses bits de qualquer forma.
    avoid_zero(avalanche_bits(hash) & MASK_HASH)
}

/// `StringHasher::computeHashAndMaskTop8Bits<T>` com o `DefaultConverter`.
pub fn compute_hash_and_mask_top8_bits<T: CharType>(data: &[T]) -> u32 {
    compute_hash_and_mask_top8_bits_with::<T, DefaultConverter>(data)
}

/// `StringHasher::computeHashAndMaskTop8Bits<T, Converter>`.
pub fn compute_hash_and_mask_top8_bits_with<T: CharType, C: Converter>(data: &[T]) -> u32 {
    avoid_zero((rapidhash::<T, C>(data) as u32) & MASK_HASH)
}

/// `StringHasher::computeLiteralHashAndMaskTop8Bits`: os caracteres sem o terminador nulo (o
/// `ASCIILiteral` entra como `&[u8]`).
pub fn compute_literal_hash_and_mask_top8_bits<T: CharType>(characters: &[T]) -> u32 {
    compute_hash_and_mask_top8_bits::<T>(characters)
}

/// Despacho do `RapidHash::rapidhash`, com a semente do WebKit:
/// - 1 byte por caractere: cada byte é o caractere truncado;
/// - 2 bytes com `DefaultConverter`: se tudo é Latin1, um byte por caractere (o byte baixo); senão
///   os `2N` bytes little-endian crus;
/// - 2 bytes com conversor próprio: sempre um byte por caractere, o OR-fold `(conv & 0xFF) | (conv >> 8)`.
fn rapidhash<T: CharType, C: Converter>(data: &[T]) -> u64 {
    let unit = |index: usize| -> u32 { data[index].into() };
    if T::SIZE == 2 && C::IS_DEFAULT && !data.iter().all(|c| Into::<u32>::into(*c) <= 0xFF) {
        return rapidhash_by(WEBKIT_SEED, 2 * data.len(), |index| (unit(index >> 1) >> (8 * (index & 1))) as u8);
    }
    rapidhash_by(WEBKIT_SEED, data.len(), |index| {
        if C::IS_DEFAULT {
            unit(index) as u8
        } else {
            let conv = C::convert(unit(index) as u16);
            ((conv & 0xFF) | (conv >> 8)) as u8
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Referência independente: o rapidhash do repositório original sobre um vetor de bytes, com
    /// leituras `from_le_bytes`.
    #[test]
    fn latin1_utf16_equals_latin1_bytes() {
        for len in 0..=120usize {
            let wide: Vec<u16> = (0..len).map(|i| ((i * 13 + 1) & 0xFF) as u16).collect();
            let narrow: Vec<u8> = wide.iter().map(|c| *c as u8).collect();
            assert_eq!(
                compute_hash_and_mask_top8_bits::<u16>(&wide),
                compute_hash_and_mask_top8_bits::<u8>(&narrow),
                "len {len}"
            );
        }
    }

    #[test]
    fn hash_never_zero_and_top_bits_clear() {
        for len in 0..64usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 5) as u8).collect();
            let h = compute_hash_and_mask_top8_bits::<u8>(&data);
            assert_ne!(h, 0);
            assert_eq!(h >> 24, 0);
        }
        assert_eq!(avoid_zero(0), 0x0080_0000);
        assert_eq!(avoid_zero(5), 5);
    }

    #[test]
    fn literal_equals_runtime_hash() {
        assert_eq!(
            compute_literal_hash_and_mask_top8_bits::<u8>(b"length"),
            compute_hash_and_mask_top8_bits::<u8>(b"length")
        );
    }

    /// O texto de 16 bits do caminho padrão é o mesmo hash de texto do WTF que o `ul_common` dá.
    #[test]
    fn default_converter_is_the_wtf_text_hash() {
        for text in ["", "a", "length", "ação", "中文", "😀 emoji"] {
            let units: Vec<u16> = text.encode_utf16().collect();
            assert_eq!(compute_hash_and_mask_top8_bits::<u16>(&units), crate::wtf::rapidhash::wtf_string_hash(WEBKIT_SEED, &units), "{text}");
        }
    }

    #[test]
    fn avalanche_matches_manual_steps() {
        // Passos do C++ aplicados à mão ao valor inicial.
        let mut r = STRING_HASHING_START_VALUE;
        r ^= r << 3;
        r = r.wrapping_add(r >> 5);
        r ^= r << 2;
        r = r.wrapping_add(r >> 15);
        r ^= r << 10;
        assert_eq!(avalanche_bits(STRING_HASHING_START_VALUE), r);
        assert_eq!(finalize(0), avoid_zero(avalanche_bits(0)));
        assert_eq!(avalanche_bits(0), 0);
        assert_eq!(finalize(0), 0x0080_0000);
    }
}
