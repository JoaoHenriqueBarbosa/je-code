//! Tradução da parte de `WTF/wtf/text/StringCommon.h` que opera sobre fatias de caracteres
//! (`std::span<const CharacterType>`): igualdade, busca direta e reversa, e as versões que ignoram a
//! caixa ASCII. É o lugar único dessa lógica (regra DRY do projeto): `string_impl`, `wtf_string` e
//! `string_view` chamam estas funções em vez de manter cópias.
//!
//! O que opera sobre `StringView` (`equalCommon`, `findCommon`, `startsWith`...) vive em
//! `string_view`, que chama estas funções com as fatias do `span8()`/`span16()`.
//!
//! Não portado, por não ter comportamento observável nem uso no motor: as variantes SIMD
//! (`find8`, `find16`, `findNaN`, `findFloat`...) que dão o mesmo resultado que o laço escalar, os
//! `copyElements` (a cópia entre larguras é `copy_characters_convert` de `string_impl`) e as
//! sobrecargas que recebem `ASCIILiteral` (o chamador passa a fatia `&[u8]`).

use crate::wtf::ascii_ctype::{is_ascii_alpha_caseless_equal, to_ascii_lower};
use crate::wtf::text::string_impl::CharType;

/// `notFound` de `StringCommon.h`.
pub const NOT_FOUND: usize = usize::MAX;

/// A unidade de código como inteiro (a promoção implícita do C++ nas comparações).
pub(crate) fn unit<T: CharType>(character: T) -> u32 {
    character.into()
}

/// `charactersAreAllASCII(span)`.
pub(crate) fn characters_are_all_ascii<T: CharType>(characters: &[T]) -> bool {
    characters.iter().all(|character| unit(*character) <= 0x7F)
}

/// `equal(const CharType* a, std::span<const OtherType> b)` e as versões de larguras mistas:
/// compara os `b.len()` primeiros caracteres de `a` com `b`. O C++ exige `a` com pelo menos `b.len()`
/// elementos; aqui um `a` mais curto dá `false`.
pub(crate) fn equal_prefix<A: CharType, B: CharType>(a: &[A], b: &[B]) -> bool {
    a.len() >= b.len() && a.iter().zip(b).all(|(x, y)| unit(*x) == unit(*y))
}

/// `equalIgnoringASCIICaseWithLength` (o laço escalar; o caminho SIMD dá o mesmo resultado).
pub(crate) fn equal_ignoring_ascii_case_with_length<A: CharType, B: CharType>(
    a: &[A],
    b: &[B],
    length_to_check: usize,
) -> bool {
    debug_assert!(a.len() >= length_to_check);
    debug_assert!(b.len() >= length_to_check);
    (0..length_to_check).all(|i| to_ascii_lower(a[i].to_u16()) == to_ascii_lower(b[i].to_u16()))
}

/// `equalLettersIgnoringASCIICaseWithLength`: `lowercase_letters` já em minúsculas (letra, dígito
/// ou pontuação de 0x21 a 0x3F).
pub(crate) fn equal_letters_ignoring_ascii_case_with_length<T: CharType>(
    characters: &[T],
    lowercase_letters: &[u8],
    length: usize,
) -> bool {
    (0..length).all(|i| is_ascii_alpha_caseless_equal(characters[i].to_u16(), lowercase_letters[i]))
}

/// `findIgnoringASCIICase(std::span, std::span, startOffset)`.
pub(crate) fn find_ignoring_ascii_case_spans<S: CharType, M: CharType>(
    source: &[S],
    match_characters: &[M],
    start_offset: usize,
) -> usize {
    let mut offset = start_offset;
    while offset <= source.len() && source.len() - offset >= match_characters.len() {
        if equal_ignoring_ascii_case_with_length(&source[offset..], match_characters, match_characters.len()) {
            return offset;
        }
        offset += 1;
    }
    NOT_FOUND
}

/// `WTF::find(span, matchFunction, start)`.
pub fn find<T: CharType>(characters: &[T], match_function: impl Fn(u16) -> bool, start: usize) -> usize {
    let mut start = start;
    while start < characters.len() {
        if match_function(characters[start].to_u16()) {
            return start;
        }
        start += 1;
    }
    NOT_FOUND
}

/// `findInner`: soma corrente dos caracteres, só chama `equal` quando as somas coincidem.
/// O chamador garante `search_characters.len() >= match_characters.len()`.
pub(crate) fn find_inner<S: CharType, M: CharType>(
    search_characters: &[S],
    match_characters: &[M],
    index: usize,
) -> usize {
    // delta is the number of additional times to test; delta == 0 means test only once.
    let delta = search_characters.len() - match_characters.len();

    let mut search_hash: u32 = 0;
    let mut match_hash: u32 = 0;

    for i in 0..match_characters.len() {
        search_hash = search_hash.wrapping_add(unit(search_characters[i]));
        match_hash = match_hash.wrapping_add(unit(match_characters[i]));
    }

    let mut i = 0;
    // keep looping until we match
    while search_hash != match_hash || !equal_prefix(&search_characters[i..], match_characters) {
        if i == delta {
            return NOT_FOUND;
        }
        search_hash = search_hash.wrapping_add(unit(search_characters[i + match_characters.len()]));
        search_hash = search_hash.wrapping_sub(unit(search_characters[i]));
        i += 1;
    }
    index + i
}

/// `reverseFindInner`.
pub(crate) fn reverse_find_inner<S: CharType, M: CharType>(
    search_characters: &[S],
    match_characters: &[M],
    start: usize,
) -> usize {
    if search_characters.len() < match_characters.len() {
        return NOT_FOUND;
    }

    // delta is the number of additional times to test; delta == 0 means test only once.
    let mut delta = std::cmp::min(start, search_characters.len() - match_characters.len());

    let mut search_hash: u32 = 0;
    let mut match_hash: u32 = 0;
    for i in 0..match_characters.len() {
        search_hash = search_hash.wrapping_add(unit(search_characters[delta + i]));
        match_hash = match_hash.wrapping_add(unit(match_characters[i]));
    }

    // keep looping until we match
    while search_hash != match_hash || !equal_prefix(&search_characters[delta..], match_characters) {
        if delta == 0 {
            return NOT_FOUND;
        }
        delta -= 1;
        search_hash = search_hash.wrapping_sub(unit(search_characters[delta + match_characters.len()]));
        search_hash = search_hash.wrapping_add(unit(search_characters[delta]));
    }
    delta
}

/// `WTF::reverseFind(span, matchCharacter, start)`; `start` padrão no C++ é `MaxLength`.
pub fn reverse_find<T: CharType>(characters: &[T], match_character: T, start: usize) -> usize {
    if characters.is_empty() {
        return NOT_FOUND;
    }
    let mut start = start;
    if start >= characters.len() {
        start = characters.len() - 1;
    }
    let search_length = start + 1;
    match characters[..search_length].iter().rposition(|c| *c == match_character) {
        Some(index) => index,
        None => NOT_FOUND,
    }
}

/// `WTF::reverseFind(span<const char16_t>, Latin1Character, start)`.
pub fn reverse_find_16_latin1(characters: &[u16], match_character: u8, start: usize) -> usize {
    reverse_find(characters, match_character as u16, start)
}

/// `WTF::reverseFind(span<const Latin1Character>, char16_t, start)`.
pub fn reverse_find_8_char16(characters: &[u8], match_character: u16, start: usize) -> usize {
    if match_character > 0xFF {
        return NOT_FOUND;
    }
    reverse_find(characters, match_character as u8, start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_prefix_mixed_widths() {
        assert!(equal_prefix(b"abc".as_slice(), &[0x61u16, 0x62]));
        assert!(!equal_prefix(b"ab".as_slice(), &[0x61u16, 0x62, 0x63]));
        assert!(!equal_prefix(&[0x20ACu16], b"x".as_slice()));
        assert!(equal_prefix(b"".as_slice(), b"".as_slice()));
    }

    #[test]
    fn ignoring_ascii_case_helpers() {
        assert!(equal_ignoring_ascii_case_with_length(b"AbC".as_slice(), &[0x61u16, 0x42, 0x63], 3));
        assert!(!equal_ignoring_ascii_case_with_length(b"AbC".as_slice(), b"abd".as_slice(), 3));
        assert_eq!(find_ignoring_ascii_case_spans(b"xxHeLLo".as_slice(), b"hello".as_slice(), 0), 2);
        assert_eq!(find_ignoring_ascii_case_spans(b"xxHeLLo".as_slice(), b"hello".as_slice(), 3), NOT_FOUND);
        assert!(equal_letters_ignoring_ascii_case_with_length(b"HeLLo".as_slice(), b"hello", 5));
    }

    #[test]
    fn find_and_reverse_find_inner() {
        assert_eq!(find_inner(b"hello world".as_slice(), b"o w".as_slice(), 0), 4);
        assert_eq!(find_inner(b"hello".as_slice(), &[0x20ACu16], 0), NOT_FOUND);
        assert_eq!(reverse_find_inner(b"abcabc".as_slice(), b"bc".as_slice(), usize::MAX), 4);
        assert_eq!(reverse_find_inner(b"abcabc".as_slice(), b"bc".as_slice(), 3), 1);
        assert_eq!(find(b"a-b".as_slice(), |c| c == '-' as u16, 0), 1);
        assert_eq!(reverse_find(b"a\nb\r".as_slice(), b'\n', usize::MAX), 1);
        assert_eq!(reverse_find_8_char16(b"abc".as_slice(), 0x20AC, usize::MAX), NOT_FOUND);
        assert_eq!(reverse_find_16_latin1(&[0x61u16, 0x62], b'a', usize::MAX), 0);
    }
}
