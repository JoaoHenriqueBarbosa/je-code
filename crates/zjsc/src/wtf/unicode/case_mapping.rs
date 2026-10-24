//! O pedaço do ICU que a WTF usa para mudar a caixa de letras (`u_tolower`, `u_toupper`,
//! `u_strToLower`, `u_strToUpper` na localidade raiz), sobre as tabelas do UCD 17.0 que o ICU 78.3
//! do bun 1.4.2 traz. As tabelas saem de `scripts/gen-case-mapping.py`.

use super::case_mapping_tables::{
    CASED, CASE_IGNORABLE, FULL_LOWER, FULL_UPPER, SIMPLE_LOWER, SIMPLE_UPPER,
};

const GREEK_CAPITAL_SIGMA: u32 = 0x03A3;
const GREEK_SMALL_SIGMA: u32 = 0x03C3;
const GREEK_SMALL_FINAL_SIGMA: u32 = 0x03C2;

fn lookup(table: &[(u32, u32)], c: u32) -> Option<u32> {
    table.binary_search_by_key(&c, |&(k, _)| k).ok().map(|i| table[i].1)
}

fn in_ranges(table: &[(u32, u32)], c: u32) -> bool {
    match table.binary_search_by(|&(a, b)| {
        if b < c {
            std::cmp::Ordering::Less
        } else if a > c {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    }) {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// `u_tolower`: mapeamento simples, um ponto de código para um.
pub fn to_lower(c: u32) -> u32 {
    lookup(SIMPLE_LOWER, c).unwrap_or(c)
}

/// `u_toupper`: mapeamento simples, um ponto de código para um.
pub fn to_upper(c: u32) -> u32 {
    lookup(SIMPLE_UPPER, c).unwrap_or(c)
}

/// Lê o ponto de código que começa em `i`, juntando um par de surrogates válido.
fn code_point_at(s: &[u16], i: usize) -> (u32, usize) {
    let c = s[i] as u32;
    if (0xD800..0xDC00).contains(&c) && i + 1 < s.len() {
        let d = s[i + 1] as u32;
        if (0xDC00..0xE000).contains(&d) {
            return (0x10000 + ((c - 0xD800) << 10) + (d - 0xDC00), 2);
        }
    }
    (c, 1)
}

/// Lê o ponto de código que termina antes de `end`.
fn code_point_before(s: &[u16], end: usize) -> (u32, usize) {
    let d = s[end - 1] as u32;
    if (0xDC00..0xE000).contains(&d) && end >= 2 {
        let c = s[end - 2] as u32;
        if (0xD800..0xDC00).contains(&c) {
            return (0x10000 + ((c - 0xD800) << 10) + (d - 0xDC00), 2);
        }
    }
    (d, 1)
}

fn push(out: &mut Vec<u16>, c: u32) {
    if c >= 0x10000 {
        let v = c - 0x10000;
        out.push((0xD800 + (v >> 10)) as u16);
        out.push((0xDC00 + (v & 0x3FF)) as u16);
    } else {
        out.push(c as u16);
    }
}

/// A condição Final_Sigma do SpecialCasing: antes do sigma há uma letra com caixa (pulando as
/// ignoráveis), e depois dele não há.
fn is_final_sigma(s: &[u16], start: usize, end: usize) -> bool {
    let mut i = start;
    let mut before = false;
    while i > 0 {
        let (c, n) = code_point_before(s, i);
        i -= n;
        if in_ranges(CASE_IGNORABLE, c) {
            continue;
        }
        before = in_ranges(CASED, c);
        break;
    }
    if !before {
        return false;
    }
    let mut j = end;
    while j < s.len() {
        let (c, n) = code_point_at(s, j);
        j += n;
        if in_ranges(CASE_IGNORABLE, c) {
            continue;
        }
        return !in_ranges(CASED, c);
    }
    true
}

/// `u_strToLower` na localidade raiz: mapeamento completo, com o Final_Sigma.
pub fn str_to_lower(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let (c, n) = code_point_at(s, i);
        if c == GREEK_CAPITAL_SIGMA {
            push(
                &mut out,
                if is_final_sigma(s, i, i + n) { GREEK_SMALL_FINAL_SIGMA } else { GREEK_SMALL_SIGMA },
            );
        } else if let Ok(k) = FULL_LOWER.binary_search_by_key(&c, |&(k, _)| k) {
            for &m in FULL_LOWER[k].1 {
                push(&mut out, m);
            }
        } else {
            push(&mut out, to_lower(c));
        }
        i += n;
    }
    out
}

/// `u_strToUpper` na localidade raiz: mapeamento completo.
pub fn str_to_upper(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let (c, n) = code_point_at(s, i);
        if let Ok(k) = FULL_UPPER.binary_search_by_key(&c, |&(k, _)| k) {
            for &m in FULL_UPPER[k].1 {
                push(&mut out, m);
            }
        } else {
            push(&mut out, to_upper(c));
        }
        i += n;
    }
    out
}

/// `u_foldCase(c, U_FOLD_CASE_DEFAULT)`: dobra simples (CaseFolding C e S).
pub fn fold_case(c: u32) -> u32 {
    lookup(super::case_mapping_tables::SIMPLE_FOLD, c).unwrap_or(c)
}

/// `u_strFoldCase(..., U_FOLD_CASE_DEFAULT)`: dobra completa (CaseFolding C e F).
pub fn str_fold_case(s: &[u16]) -> Vec<u16> {
    use super::case_mapping_tables::FULL_FOLD;
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let (c, n) = code_point_at(s, i);
        if let Ok(k) = FULL_FOLD.binary_search_by_key(&c, |&(k, _)| k) {
            for &m in FULL_FOLD[k].1 {
                push(&mut out, m);
            }
        } else {
            push(&mut out, c);
        }
        i += n;
    }
    out
}
