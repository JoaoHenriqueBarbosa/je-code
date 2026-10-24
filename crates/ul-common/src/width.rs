//! Largura de exibição de texto em colunas de terminal, como o `wcwidth`/`wcswidth` da glibc 2.41 em
//! C.UTF-8 (Unicode 16): controle não tem largura (-1), combinante e formatador ocupam 0, ideograma e
//! emoji largo ocupam 2, o resto 1.
//!
//! A tabela (`width_table.rs`) é a da própria glibc do Debian 13, tirada ponto a ponto do oráculo; a
//! crate `unicode-width` diverge dela em mais de 800 faixas (ponto não atribuído é -1 na glibc, `U+00AD`
//! e as marcas numéricas árabes são 1, e assim por diante).

use crate::width_table::{NONPRINTABLE, WIDE, ZERO};

fn in_ranges(table: &[(u32, u32)], cp: u32) -> bool {
    table.binary_search_by(|&(a, b)| if b < cp { std::cmp::Ordering::Less } else if a > cp { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal }).is_ok()
}

/// `wcwidth` de um ponto de código, como a glibc: -1 pra controle, ponto não atribuído, substituto e
/// o que passa de `U+10FFFF`; 0 pra combinante e formatador; 2 pra ideograma e emoji largo; 1 pro resto.
pub fn wcwidth_cp(cp: u32) -> i32 {
    if cp > 0x10ffff || in_ranges(&NONPRINTABLE, cp) {
        -1
    } else if in_ranges(&ZERO, cp) {
        0
    } else if in_ranges(&WIDE, cp) {
        2
    } else {
        1
    }
}

/// [`wcwidth_cp`] de um `char`.
pub fn wcwidth(c: char) -> i32 {
    wcwidth_cp(c as u32)
}

/// Largura de exibição de um texto UTF-8 em C.UTF-8 (o `wcswidth` da glibc): caractere de controle
/// conta 0 aqui, quem precisa de outra regra trata antes.
pub fn display_width(s: &str) -> usize {
    s.chars().map(|c| wcwidth(c).max(0) as usize).sum()
}

/// Largura de exibição de bytes: sequência UTF-8 inválida conta um por byte, como o `mbsnwidth` do
/// gnulib com `MBSW_ACCEPT_INVALID`.
pub fn display_width_bytes(b: &[u8]) -> usize {
    b.utf8_chunks().map(|chunk| display_width(chunk.valid()) + chunk.invalid().len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_and_latin() {
        assert_eq!(wcwidth('a'), 1);
        assert_eq!(wcwidth('~'), 1);
        assert_eq!(wcwidth('ç'), 1);
        assert_eq!(wcwidth('\u{ad}'), 1);
        assert_eq!(display_width("ção"), 3);
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn controls_have_no_width() {
        assert_eq!(wcwidth('\0'), 0);
        assert_eq!(wcwidth('\n'), -1);
        assert_eq!(wcwidth('\u{7f}'), -1);
        assert_eq!(wcwidth('\u{9f}'), -1);
        assert_eq!(wcwidth('\u{a0}'), 1);
        assert_eq!(display_width("a\tb"), 2);
    }

    #[test]
    fn combining_and_formatting_are_zero() {
        assert_eq!(wcwidth('\u{301}'), 0);
        assert_eq!(wcwidth('\u{e31}'), 0);
        assert_eq!(wcwidth('\u{200b}'), 0);
        assert_eq!(wcwidth('\u{200d}'), 0);
        assert_eq!(wcwidth('\u{fe0f}'), 0);
        assert_eq!(wcwidth('\u{1161}'), 0);
        assert_eq!(wcwidth('\u{600}'), 1);
        assert_eq!(wcwidth_cp(0x378), -1);
        assert_eq!(display_width("e\u{301}"), 1);
    }

    #[test]
    fn wide_and_fullwidth_are_two() {
        assert_eq!(wcwidth('日'), 2);
        assert_eq!(wcwidth('한'), 2);
        assert_eq!(wcwidth('\u{3000}'), 2);
        assert_eq!(wcwidth('Ａ'), 2);
        assert_eq!(wcwidth('\u{1f680}'), 2);
        assert_eq!(wcwidth('\u{2705}'), 2);
        assert_eq!(wcwidth('\u{20000}'), 2);
        assert_eq!(display_width("日本"), 4);
    }

    #[test]
    fn long_dashes_are_one() {
        assert_eq!(wcwidth('\u{2e3a}'), 1);
        assert_eq!(wcwidth('\u{2e3b}'), 1);
    }

    #[test]
    fn code_points() {
        assert_eq!(wcwidth_cp(0x4e00), 2);
        assert_eq!(wcwidth_cp(0x41), 1);
        assert_eq!(wcwidth_cp(0xd800), -1);
        assert_eq!(wcwidth_cp(0x11_0000), -1);
    }

    #[test]
    fn bytes_count_invalid_one_each() {
        assert_eq!(display_width_bytes(b"a\xffb"), 3);
        assert_eq!(display_width_bytes("日本".as_bytes()), 4);
        assert_eq!(display_width_bytes(b"\xe6\x97"), 2);
        assert_eq!(display_width_bytes(b""), 0);
    }
}
