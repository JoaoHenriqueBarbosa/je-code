//! Porte de `src/vs/base/common/strings.ts`: utilitários de texto do VS Code.
//!
//! Texto segue a semântica do JavaScript: índices, comprimentos e `charCodeAt` são em unidades UTF-16,
//! por isso as funções recebem `&[u16]` e devolvem fatias do mesmo tipo (ou [`JsString`] quando o
//! resultado é texto novo). Parâmetros que no TypeScript têm valor padrão são obrigatórios aqui, e
//! onde o original devolve `-1` por "não achei" o porte devolve `Option`.

// Pendente: o que depende de um motor de regex com semântica ECMAScript, que o projeto ainda não tem,
// ficou de fora para não inventar uma aproximação:
// - `createRegExp` e `RegExpOptions` (montam e devolvem um `RegExp`);
// - `regExpLeadsToEndlessLoop` (recebe um `RegExp` e executa `exec('')`);
// - `indexOfPattern` (recebe um `RegExp`);
// - `replaceAsync` (recebe um `RegExp`, itera `matchAll` e aceita callback assíncrono);
// - `UNUSUAL_LINE_TERMINATORS` (constante exportada do tipo `RegExp`; o uso dela, a função
//   `containsUnusualLineTerminators`, está portado como laço sobre as unidades).
// As expressões regulares fixas internas (`\b`, `CONTROL_SEQUENCES`, `/\\./g`, `{n}`) viraram
// reconhecedores escritos à mão com a mesma classe de caracteres e a mesma ordem de tentativas.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use crate::JsString;
use crate::char_code as cc;
use crate::js_string::{
    char_code_at, code_point_at, ends_with_at, index_of, is_js_whitespace, starts_with_at, substring, trim_end,
    trim_start,
};
use crate::strings_tables as tables;

/// `Ellipsis`: `…` (U+2026).
pub const ELLIPSIS: [u16; 1] = [0x2026];

/// `noBreakWhitespace`: U+00A0.
pub const NO_BREAK_WHITESPACE: [u16; 1] = [cc::NoBreakSpace];

/// `UTF8_BOM_CHARACTER`: U+FEFF.
pub const UTF8_BOM_CHARACTER: [u16; 1] = [cc::UTF8_BOM];

/// O `NaN` do JavaScript no lugar de um code point: `getNextCodePoint` e `getPrevCodePoint` o devolvem
/// quando o índice está fora do texto, e `getGraphemeBreakType` tem um caminho próprio para ele.
pub const NAN_CODE_POINT: u32 = u32::MAX;

/// `Constants.UNICODE_SUPPLEMENTARY_PLANE_BEGIN` de `uint.ts`.
const UNICODE_SUPPLEMENTARY_PLANE_BEGIN: u32 = 0x10000;

/// `CodePoint` (enum constante, privado no original).
mod code_point {
    pub const ZWJ: u32 = 0x200D;
    /// Variation Selector-16 (VS16).
    pub const EMOJI_VARIANT_SELECTOR: u32 = 0xFE0F;
    /// Combining Enclosing Keycap.
    pub const ENCLOSING_KEY_CAP: u32 = 0x20E3;
    pub const SPACE: u32 = 0x0020;
}

/// `isFalsyOrWhitespace`: `None` é o `undefined` do original.
pub fn is_falsy_or_whitespace(str: Option<&[u16]>) -> bool {
    str.is_none_or(|s| trim_start(s).is_empty())
}

/// `format`: troca `{n}` pelo argumento de índice `n`. Os argumentos chegam já convertidos em texto
/// (o `String(arg)` do JavaScript); um índice fora da lista deixa o marcador como está.
pub fn format<T: AsRef<[u16]>>(value: &[u16], args: &[T]) -> JsString {
    if args.is_empty() {
        return JsString::from(value);
    }
    let mut out = JsString::new();
    let mut i = 0;
    while i < value.len() {
        if value[i] == cc::OpenCurlyBrace {
            let digits = value[i + 1..].iter().take_while(|&&u| is_ascii_digit(u)).count();
            if digits > 0 && value.get(i + 1 + digits) == Some(&cc::CloseCurlyBrace) {
                let index = value[i + 1..i + 1 + digits]
                    .iter()
                    .fold(0usize, |acc, &u| acc.saturating_mul(10).saturating_add(usize::from(u - cc::Digit0)));
                let end = i + digits + 2;
                match args.get(index) {
                    Some(arg) => out.push_units(arg.as_ref()),
                    None => out.push_units(&value[i..end]),
                }
                i = end;
                continue;
            }
        }
        out.push_unit(value[i]);
        i += 1;
    }
    out
}

/// `format2`: como `format`, mas com nomes (`{nome}`) em vez de posições. Os valores chegam já
/// convertidos em texto; um valor `null` ou `undefined` no original equivale a não ter a chave.
/// (O original também enxerga propriedades herdadas de `Object.prototype`, como `{constructor}`;
/// aqui só valem as chaves do mapa.)
pub fn format2(template: &[u16], values: &HashMap<JsString, JsString>) -> JsString {
    if values.is_empty() {
        return JsString::from(template);
    }
    let mut out = JsString::new();
    let mut i = 0;
    while i < template.len() {
        if template[i] == cc::OpenCurlyBrace
            && let Some(len) = template[i + 1..].iter().position(|&u| u == cc::CloseCurlyBrace)
            && len > 0
        {
            let end = i + len + 2;
            match values.get(&template[i + 1..i + 1 + len]) {
                Some(value) => out.push_units(value.units()),
                None => out.push_units(&template[i..end]),
            }
            i = end;
            continue;
        }
        out.push_unit(template[i]);
        i += 1;
    }
    out
}

/// Troca cada unidade pelo texto que `entity` devolve, quando devolve.
fn replace_units(text: &[u16], entity: impl Fn(u16) -> Option<&'static str>) -> JsString {
    let mut out = JsString::new();
    for &unit in text {
        match entity(unit) {
            Some(replacement) => out.push_units(&replacement.encode_utf16().collect::<Vec<_>>()),
            None => out.push_unit(unit),
        }
    }
    out
}

/// `htmlAttributeEncodeValue`: calcula o `$val` tal que `attr` em `<div attr="$val" />` vale `value`
/// em tempo de execução, o que evita injeção de XSS.
pub fn html_attribute_encode_value(value: &[u16]) -> JsString {
    replace_units(value, |unit| match unit {
        cc::LessThan => Some("&lt;"),
        cc::GreaterThan => Some("&gt;"),
        cc::DoubleQuote => Some("&quot;"),
        cc::SingleQuote => Some("&apos;"),
        cc::Ampersand => Some("&amp;"),
        _ => None,
    })
}

/// `escape`: troca os caracteres HTML por entidades, para o texto poder ir num `innerHTML`.
pub fn escape(html: &[u16]) -> JsString {
    replace_units(html, |unit| match unit {
        cc::LessThan => Some("&lt;"),
        cc::GreaterThan => Some("&gt;"),
        cc::Ampersand => Some("&amp;"),
        _ => None,
    })
}

/// `escapeRegExpCharacters`: põe `\` antes de cada caractere especial de regex.
pub fn escape_reg_exp_characters(value: &[u16]) -> JsString {
    let mut out = JsString::new();
    for &unit in value {
        if unit_in(Some(&unit), b"\\{}*+?|^$.[]()") {
            out.push_unit(cc::Backslash);
        }
        out.push_unit(unit);
    }
    out
}

/// `count`: quantas vezes `substr` ocorre em `value`, sem sobreposição. Com `substr` vazio o original
/// entra em laço infinito; aqui devolve 0.
pub fn count(value: &[u16], substr: &[u16]) -> usize {
    if substr.is_empty() {
        return 0;
    }
    let mut result = 0;
    let mut index = index_of(value, substr, 0);
    while let Some(found) = index {
        result += 1;
        index = index_of(value, substr, found + substr.len());
    }
    result
}

/// `truncate`.
pub fn truncate(value: &[u16], max_length: usize, suffix: &[u16]) -> JsString {
    if value.len() <= max_length {
        return JsString::from(value);
    }
    let mut out = JsString::from(&value[..max_length]);
    out.push_units(suffix);
    out
}

/// `truncateMiddle`: não corta um par de surrogates ao meio.
pub fn truncate_middle(value: &[u16], max_length: usize, suffix: &[u16]) -> JsString {
    if value.len() <= max_length {
        return JsString::from(value);
    }
    let half_suffix = suffix.len() as f64 / 2.0;
    let mut prefix_length = ((max_length as f64 / 2.0).ceil() - half_suffix).floor() as i64;
    let suffix_length = ((max_length as f64 / 2.0).floor() - half_suffix).ceil() as i64;
    let mut suffix_start = value.len() as i64 - suffix_length;

    if is_high_surrogate_at(value, prefix_length - 1) && is_low_surrogate_at(value, prefix_length) {
        prefix_length -= 1;
    }
    if is_high_surrogate_at(value, suffix_start - 1) && is_low_surrogate_at(value, suffix_start) {
        suffix_start += 1;
    }

    let mut out = JsString::from(substring(value, 0, prefix_length));
    out.push_units(suffix);
    out.push_units(substring(value, suffix_start, value.len() as i64));
    out
}

fn is_high_surrogate_at(value: &[u16], index: i64) -> bool {
    char_code_at(value, index).is_some_and(is_high_surrogate)
}

fn is_low_surrogate_at(value: &[u16], index: i64) -> bool {
    char_code_at(value, index).is_some_and(is_low_surrogate)
}

/// `trim`: tira todas as ocorrências de `needle` do começo e do fim de `haystack`.
pub fn trim<'a>(haystack: &'a [u16], needle: &[u16]) -> &'a [u16] {
    rtrim(ltrim(haystack, needle), needle)
}

/// `ltrim`: tira todas as ocorrências de `needle` do começo de `haystack`.
pub fn ltrim<'a>(haystack: &'a [u16], needle: &[u16]) -> &'a [u16] {
    if haystack.is_empty() || needle.is_empty() {
        return haystack;
    }
    let mut offset = 0;
    while starts_with_at(haystack, needle, offset) {
        offset += needle.len();
    }
    &haystack[offset..]
}

/// `rtrim`: tira todas as ocorrências de `needle` do fim de `haystack`.
pub fn rtrim<'a>(haystack: &'a [u16], needle: &[u16]) -> &'a [u16] {
    if haystack.is_empty() || needle.is_empty() {
        return haystack;
    }
    let mut offset = haystack.len();
    while offset > 0 && ends_with_at(haystack, needle, offset) {
        offset -= needle.len();
    }
    &haystack[..offset]
}

/// `convertSimple2RegExpPattern`: escapa os caracteres especiais e troca `*` por `.*`.
pub fn convert_simple2_reg_exp_pattern(pattern: &[u16]) -> JsString {
    let mut out = JsString::new();
    for &unit in pattern {
        if unit == cc::Asterisk {
            out.push_units(&[cc::Period, cc::Asterisk]);
            continue;
        }
        if unit_in(Some(&unit), b"-\\{}+?|^$.,[]()#") || is_js_whitespace(unit) {
            out.push_unit(cc::Backslash);
        }
        out.push_unit(unit);
    }
    out
}

/// `joinStrings`: `None` representa `undefined`, `null` e `false`, que o original descarta.
pub fn join_strings(items: &[Option<&[u16]>], separator: &[u16]) -> JsString {
    let mut out = JsString::new();
    for (position, item) in items.iter().flatten().enumerate() {
        if position > 0 {
            out.push_units(separator);
        }
        out.push_units(item);
    }
    out
}

/// Fim da quebra de linha (`\r\n`, `\r` ou `\n`) que começa em `index`, se houver.
fn line_break_end(str: &[u16], index: usize) -> Option<usize> {
    match str.get(index)? {
        &cc::CarriageReturn if str.get(index + 1) == Some(&cc::LineFeed) => Some(index + 2),
        &cc::CarriageReturn | &cc::LineFeed => Some(index + 1),
        _ => None,
    }
}

/// `splitLines`.
pub fn split_lines(str: &[u16]) -> Vec<&[u16]> {
    split_lines_with(str, false)
}

/// `splitLinesIncludeSeparators`.
pub fn split_lines_include_separators(str: &[u16]) -> Vec<&[u16]> {
    split_lines_with(str, true)
}

fn split_lines_with(str: &[u16], include_separators: bool) -> Vec<&[u16]> {
    let mut lines = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < str.len() {
        match line_break_end(str, i) {
            Some(end) => {
                lines.push(&str[start..if include_separators { end } else { i }]);
                start = end;
                i = end;
            }
            None => i += 1,
        }
    }
    lines.push(&str[start..]);
    lines
}

fn is_space_or_tab(unit: Option<u16>) -> bool {
    matches!(unit, Some(cc::Space | cc::Tab))
}

/// `firstNonWhitespaceIndex`: primeiro índice que não é espaço nem tab, ou `None` se não houver.
pub fn first_non_whitespace_index(str: &[u16]) -> Option<usize> {
    str.iter().position(|&u| !is_space_or_tab(Some(u)))
}

/// `getLeadingWhitespace`: o espaço em branco (espaços e tabs) no começo de `str[start..end]`. Se
/// houver só espaço em branco, devolve o trecho todo.
pub fn get_leading_whitespace(str: &[u16], start: usize, end: usize) -> &[u16] {
    for i in start..end {
        if !is_space_or_tab(char_code_at(str, i as i64)) {
            return substring(str, start as i64, i as i64);
        }
    }
    substring(str, start as i64, end as i64)
}

/// `lastNonWhitespaceIndex`: último índice, a partir de `start_index` (o original usa `length - 1`),
/// que não é espaço nem tab, ou `None` se não houver. Um índice além do fim do texto conta como
/// "não é espaço", como o `NaN` do `charCodeAt`.
pub fn last_non_whitespace_index(str: &[u16], start_index: i64) -> Option<usize> {
    (0..=start_index).rev().find(|&i| !is_space_or_tab(char_code_at(str, i))).map(|i| i as usize)
}

/// `getIndentationLength`.
pub fn get_indentation_length(str: &[u16]) -> usize {
    first_non_whitespace_index(str).unwrap_or(str.len())
}

/// `compare`: ordem por unidade UTF-16, devolvida como -1, 0 ou 1.
pub fn compare(a: &[u16], b: &[u16]) -> i32 {
    a.cmp(b) as i32
}

/// `compareSubstring`.
pub fn compare_substring(a: &[u16], b: &[u16], a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> i32 {
    let (mut a_start, mut b_start) = (a_start, b_start);
    while a_start < a_end && b_start < b_end {
        let (code_a, code_b) = (a[a_start], b[b_start]);
        if code_a != code_b {
            return if code_a < code_b { -1 } else { 1 };
        }
        a_start += 1;
        b_start += 1;
    }
    compare_remaining(a_end as i64 - a_start as i64, b_end as i64 - b_start as i64)
}

fn compare_remaining(a_len: i64, b_len: i64) -> i32 {
    a_len.cmp(&b_len) as i32
}

/// `compareIgnoreCase`.
pub fn compare_ignore_case(a: &[u16], b: &[u16]) -> i32 {
    compare_substring_ignore_case(a, b, 0, a.len(), 0, b.len())
}

/// `compareSubstringIgnoreCase`: letras ASCII comparam sem distinguir caixa; qualquer unidade acima
/// de 127 manda o resto do trecho para a comparação em minúsculas.
pub fn compare_substring_ignore_case(
    a: &[u16],
    b: &[u16],
    a_start: usize,
    a_end: usize,
    b_start: usize,
    b_end: usize,
) -> i32 {
    let (mut a_start, mut b_start) = (a_start, b_start);
    while a_start < a_end && b_start < b_end {
        let (code_a, code_b) = (a[a_start], b[b_start]);
        if code_a != code_b {
            if code_a >= 128 || code_b >= 128 {
                // Fora do ASCII: volta a comparar o texto convertido para minúsculas.
                let lower_a = JsString::from(substring(a, a_start as i64, a_end as i64)).to_lower_case();
                let lower_b = JsString::from(substring(b, b_start as i64, b_end as i64)).to_lower_case();
                return compare(lower_a.units(), lower_b.units());
            }
            // Letras minúsculas ASCII [97-122] sobem para as maiúsculas [65-90].
            let fold = |code: u16| i32::from(if is_lower_ascii_letter(code) { code - 32 } else { code });
            let diff = fold(code_a) - fold(code_b);
            if diff != 0 {
                return diff;
            }
        }
        a_start += 1;
        b_start += 1;
    }
    compare_remaining(a_end as i64 - a_start as i64, b_end as i64 - b_start as i64)
}

/// `isAsciiDigit`.
pub fn is_ascii_digit(code: u16) -> bool {
    (cc::Digit0..=cc::Digit9).contains(&code)
}

/// `isLowerAsciiLetter`.
pub fn is_lower_ascii_letter(code: u16) -> bool {
    (cc::a..=cc::z).contains(&code)
}

/// `isUpperAsciiLetter`.
pub fn is_upper_ascii_letter(code: u16) -> bool {
    (cc::A..=cc::Z).contains(&code)
}

/// `equalsIgnoreCase`.
pub fn equals_ignore_case(a: &[u16], b: &[u16]) -> bool {
    a.len() == b.len() && compare_substring_ignore_case(a, b, 0, a.len(), 0, b.len()) == 0
}

/// `equals`: `None` é o `undefined` do original.
pub fn equals(a: Option<&[u16]>, b: Option<&[u16]>, ignore_case: bool) -> bool {
    a == b || (ignore_case && a.zip(b).is_some_and(|(a, b)| equals_ignore_case(a, b)))
}

/// `startsWithIgnoreCase`.
pub fn starts_with_ignore_case(str: &[u16], candidate: &[u16]) -> bool {
    let len = candidate.len();
    len <= str.len() && compare_substring_ignore_case(str, candidate, 0, len, 0, candidate.len()) == 0
}

/// `endsWithIgnoreCase`.
pub fn ends_with_ignore_case(str: &[u16], candidate: &[u16]) -> bool {
    let len = str.len();
    len.checked_sub(candidate.len())
        .is_some_and(|start| compare_substring_ignore_case(str, candidate, start, len, 0, candidate.len()) == 0)
}

/// `commonPrefixLength`: o tamanho do prefixo comum às duas cadeias.
pub fn common_prefix_length(a: &[u16], b: &[u16]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// `commonSuffixLength`: o tamanho do sufixo comum às duas cadeias.
pub fn common_suffix_length(a: &[u16], b: &[u16]) -> usize {
    a.iter().rev().zip(b.iter().rev()).take_while(|(x, y)| x == y).count()
}

/// `isHighSurrogate`.
pub fn is_high_surrogate(char_code: u16) -> bool {
    (0xD800..=0xDBFF).contains(&char_code)
}

/// `isLowSurrogate`.
pub fn is_low_surrogate(char_code: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&char_code)
}

/// `computeCodePoint`: junta um par de surrogates.
pub fn compute_code_point(high_surrogate: u16, low_surrogate: u16) -> u32 {
    ((u32::from(high_surrogate) - 0xD800) << 10) + (u32::from(low_surrogate) - 0xDC00) + 0x10000
}

/// `getNextCodePoint`: o code point que começa em `offset`, ou [`NAN_CODE_POINT`] se `offset` está
/// fora do texto. `len` e `offset` são com sinal porque o original deixa o deslocamento chegar a -1.
pub fn get_next_code_point(str: &[u16], len: i64, offset: i64) -> u32 {
    let Some(char_code) = char_code_at(str, offset) else {
        return NAN_CODE_POINT;
    };
    if is_high_surrogate(char_code)
        && offset + 1 < len
        && let Some(next) = char_code_at(str, offset + 1)
        && is_low_surrogate(next)
    {
        return compute_code_point(char_code, next);
    }
    u32::from(char_code)
}

/// `getPrevCodePoint`: o code point que termina logo antes de `offset`.
fn get_prev_code_point(str: &[u16], offset: i64) -> u32 {
    let Some(char_code) = char_code_at(str, offset - 1) else {
        return NAN_CODE_POINT;
    };
    if is_low_surrogate(char_code)
        && offset > 1
        && let Some(prev) = char_code_at(str, offset - 2)
        && is_high_surrogate(prev)
    {
        return compute_code_point(prev, char_code);
    }
    u32::from(char_code)
}

/// Quantas unidades UTF-16 ocupa o code point (o `NaN` ocupa uma, como no original).
fn code_point_width(code_point: u32) -> i64 {
    if code_point != NAN_CODE_POINT && code_point >= UNICODE_SUPPLEMENTARY_PLANE_BEGIN { 2 } else { 1 }
}

/// `CodePointIterator`.
pub struct CodePointIterator<'a> {
    str: &'a [u16],
    len: i64,
    offset: i64,
}

impl<'a> CodePointIterator<'a> {
    pub fn new(str: &'a [u16], offset: usize) -> Self {
        Self { str, len: str.len() as i64, offset: offset as i64 }
    }

    /// Com sinal: o original deixa `prevCodePoint` levar o deslocamento a -1 no começo do texto.
    pub fn offset(&self) -> i64 {
        self.offset
    }

    pub fn set_offset(&mut self, offset: i64) {
        self.offset = offset;
    }

    pub fn prev_code_point(&mut self) -> u32 {
        let code_point = get_prev_code_point(self.str, self.offset);
        self.offset -= code_point_width(code_point);
        code_point
    }

    pub fn next_code_point(&mut self) -> u32 {
        let code_point = get_next_code_point(self.str, self.len, self.offset);
        self.offset += code_point_width(code_point);
        code_point
    }

    pub fn eol(&self) -> bool {
        self.offset >= self.len
    }
}

/// `GraphemeIterator`.
pub struct GraphemeIterator<'a> {
    iterator: CodePointIterator<'a>,
}

impl<'a> GraphemeIterator<'a> {
    pub fn new(str: &'a [u16], offset: usize) -> Self {
        Self { iterator: CodePointIterator::new(str, offset) }
    }

    pub fn offset(&self) -> i64 {
        self.iterator.offset()
    }

    pub fn next_grapheme_length(&mut self) -> usize {
        let iterator = &mut self.iterator;
        let initial_offset = iterator.offset();

        let mut grapheme_break_type = get_grapheme_break_type(iterator.next_code_point());
        while !iterator.eol() {
            let offset = iterator.offset();
            let next_grapheme_break_type = get_grapheme_break_type(iterator.next_code_point());
            if break_between_grapheme_break_type(grapheme_break_type, next_grapheme_break_type) {
                // Volta o iterador.
                iterator.set_offset(offset);
                break;
            }
            grapheme_break_type = next_grapheme_break_type;
        }
        (iterator.offset() - initial_offset) as usize
    }

    pub fn prev_grapheme_length(&mut self) -> usize {
        let iterator = &mut self.iterator;
        let initial_offset = iterator.offset();

        let mut grapheme_break_type = get_grapheme_break_type(iterator.prev_code_point());
        while iterator.offset() > 0 {
            let offset = iterator.offset();
            let prev_grapheme_break_type = get_grapheme_break_type(iterator.prev_code_point());
            if break_between_grapheme_break_type(prev_grapheme_break_type, grapheme_break_type) {
                // Volta o iterador.
                iterator.set_offset(offset);
                break;
            }
            grapheme_break_type = prev_grapheme_break_type;
        }
        (initial_offset - iterator.offset()) as usize
    }

    pub fn eol(&self) -> bool {
        self.iterator.eol()
    }
}

/// `nextCharLength`.
pub fn next_char_length(str: &[u16], initial_offset: usize) -> usize {
    GraphemeIterator::new(str, initial_offset).next_grapheme_length()
}

/// `prevCharLength`.
pub fn prev_char_length(str: &[u16], initial_offset: usize) -> usize {
    GraphemeIterator::new(str, initial_offset).prev_grapheme_length()
}

/// `getCharContainingOffset`: o início e o fim do grafema que contém `offset`.
pub fn get_char_containing_offset(str: &[u16], offset: usize) -> (usize, usize) {
    let mut offset = offset;
    if offset > 0 && is_low_surrogate_at(str, offset as i64) {
        offset -= 1;
    }
    let end_offset = offset + next_char_length(str, offset);
    let start_offset = end_offset - prev_char_length(str, end_offset);
    (start_offset, end_offset)
}

/// `charCount`: quantos grafemas há no texto.
pub fn char_count(str: &[u16]) -> usize {
    let mut iterator = GraphemeIterator::new(str, 0);
    let mut length = 0;
    while !iterator.eol() {
        length += 1;
        iterator.next_grapheme_length();
    }
    length
}

/// `containsRTL`: se `str` tem algum caractere Unicode classificado como "R" ou "AL" (a classe da
/// regex original, comparada code point a code point).
pub fn contains_rtl(str: &[u16]) -> bool {
    (0..str.len()).any(|i| {
        let code_point = code_point_at(str, i).unwrap_or_default();
        tables::RTL_RANGES
            .binary_search_by(|&(first, last)| {
                if code_point < first {
                    std::cmp::Ordering::Greater
                } else if code_point > last {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    })
}

/// `isBasicASCII`: só caracteres de 32 a 126, `\n`, `\r` e `\t`.
pub fn is_basic_ascii(str: &[u16]) -> bool {
    str.iter().all(|&u| matches!(u, cc::Tab | cc::LineFeed | cc::CarriageReturn | 0x20..=0x7E))
}

/// `containsUnusualLineTerminators`: se há LINE SEPARATOR (U+2028) ou PARAGRAPH SEPARATOR (U+2029).
pub fn contains_unusual_line_terminators(str: &[u16]) -> bool {
    str.iter().any(|&u| u == cc::LINE_SEPARATOR || u == cc::PARAGRAPH_SEPARATOR)
}

/// `isFullWidthCharacter`: trata caracteres largos como duas colunas (faixas CJK, hangul, formas
/// de largura total).
pub fn is_full_width_character(char_code: u16) -> bool {
    (0x2E80..=0xD7AF).contains(&char_code)
        || (0xF900..=0xFAFF).contains(&char_code)
        || (0xFF01..=0xFF5E).contains(&char_code)
        || (0xFFE0..=0xFFE6).contains(&char_code)
}

/// `isEmojiImprecise`: checagem rápida (e por isso imprecisa) de emoji.
pub fn is_emoji_imprecise(x: u32) -> bool {
    tables::EMOJI_IMPRECISE_RANGES.iter().any(|&(first, last)| (first..=last).contains(&x))
}

/// Posições do `\b` (borda de palavra ASCII `[A-Za-z0-9_]`, sem a flag `u` nem `i`) a partir de `from`.
fn next_word_boundary(str: &[u16], from: usize) -> Option<usize> {
    let is_word = |index: Option<usize>| {
        index.and_then(|i| str.get(i)).is_some_and(|&u| {
            is_ascii_digit(u) || is_lower_ascii_letter(u) || is_upper_ascii_letter(u) || u == cc::Underline
        })
    };
    (from..=str.len()).find(|&p| is_word(p.checked_sub(1)) != is_word(Some(p)))
}

/// `lcut`: encurta o começo do texto em posições favoráveis (espaço, pontuação). O resultado pode ser
/// maior que `n`, e o espaço à esquerda sempre sai.
pub fn lcut(text: &[u16], n: usize, prefix: &[u16]) -> JsString {
    let trimmed = trim_start(text);
    if trimmed.len() < n {
        return JsString::from(trimmed);
    }

    let mut i = 0;
    let mut from = 0;
    while let Some(boundary) = next_word_boundary(trimmed, from) {
        if ((trimmed.len() - boundary) as i64) < n as i64 {
            break;
        }
        i = boundary;
        from = boundary + 1;
    }

    if i == 0 {
        return JsString::from(trimmed);
    }

    let mut out = JsString::from(prefix);
    out.push_units(trim_start(&trimmed[i..]));
    out
}

/// `rcut`: encurta o fim do texto em posições favoráveis (espaço, pontuação). O espaço à direita
/// sempre sai.
pub fn rcut(text: &[u16], n: usize, suffix: &[u16]) -> JsString {
    let trimmed = trim_end(text);
    if trimmed.len() <= n {
        return JsString::from(trimmed);
    }

    let mut last_good_break = 0;
    let mut found_boundary_after_n = false;
    let mut from = 0;
    while let Some(boundary) = next_word_boundary(trimmed, from) {
        if boundary > n {
            found_boundary_after_n = true;
            break;
        }
        last_good_break = boundary;
        from = boundary + 1;
    }

    // Sem fronteira depois de n não há onde cortar bem: devolve tudo.
    if !found_boundary_after_n {
        return JsString::from(trimmed);
    }

    // Se a única fronteira até n é o começo, cortar ali dá texto vazio: só o sufixo.
    if last_good_break == 0 {
        return JsString::from(suffix);
    }

    let result = trim_end(&trimmed[..last_good_break]);

    // Se o trimEnd tirou mais da metade do que foi cortado (cortou quase só espaço), devolve tudo.
    if (result.len() as f64) < last_good_break as f64 / 2.0 {
        return JsString::from(trimmed);
    }

    let mut out = JsString::from(result);
    out.push_units(suffix);
    out
}

// Sequências de controle (padrão de fato: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html).
// O original junta três regex numa alternância `CSI|OSC|ESC` e as varre da esquerda para a direita; os
// reconhecedores abaixo têm as mesmas classes e, como nenhuma classe opcional se sobrepõe à seguinte,
// a primeira tentativa de cada um é a única possível (o retrocesso da regex nunca muda o resultado).

const ESC: u16 = 0x1B;
const CSI_8BIT: u16 = 0x9B;
const OSC_8BIT: u16 = 0x9D;
const ST_8BIT: u16 = 0x9C;
const BEL: u16 = 0x07;

fn unit_in(unit: Option<&u16>, set: &[u8]) -> bool {
    unit.is_some_and(|&u| u < 128 && set.contains(&(u as u8)))
}

fn is_ascii_letter(unit: Option<&u16>) -> bool {
    unit.is_some_and(|&u| is_lower_ascii_letter(u) || is_upper_ascii_letter(u))
}

/// `CSI_SEQUENCE`: `(?:\x1b\[|\x9b)[=?>!]?[\d;:]*["$#'* ]?[a-zA-Z@^`{}|~]`.
fn match_csi_sequence(str: &[u16], start: usize) -> Option<usize> {
    let mut i = match (str[start], str.get(start + 1)) {
        (ESC, Some(&cc::OpenSquareBracket)) => start + 2,
        (CSI_8BIT, _) => start + 1,
        _ => return None,
    };
    if unit_in(str.get(i), b"=?>!") {
        i += 1;
    }
    while str.get(i).is_some_and(|&u| is_ascii_digit(u) || u == cc::Semicolon || u == cc::Colon) {
        i += 1;
    }
    if unit_in(str.get(i), b"\"$#'* ") {
        i += 1;
    }
    (is_ascii_letter(str.get(i)) || unit_in(str.get(i), b"@^`{}|~")).then_some(i + 1)
}

/// Procura o fim do primeiro terminador a partir de `from`, aceitando entre `from` e ele só caracteres
/// que o `.` da regex aceita (tudo menos `\n`, `\r`, U+2028 e U+2029): é o `.*?` seguido do terminador.
fn match_lazy_dot_until(str: &[u16], from: usize, terminator: impl Fn(usize) -> Option<usize>) -> Option<usize> {
    let mut j = from;
    loop {
        if let Some(len) = terminator(j) {
            return Some(j + len);
        }
        match str.get(j) {
            Some(&u) if !is_line_terminator(u) => j += 1,
            _ => return None,
        }
    }
}

fn is_line_terminator(unit: u16) -> bool {
    matches!(unit, cc::LineFeed | cc::CarriageReturn | cc::LINE_SEPARATOR | cc::PARAGRAPH_SEPARATOR)
}

/// `OSC_SEQUENCE`: `(?:\x1b\]|\x9d).*?(?:\x1b\\|\x07|\x9c)`.
fn match_osc_sequence(str: &[u16], start: usize) -> Option<usize> {
    let body = match (str[start], str.get(start + 1)) {
        (ESC, Some(&cc::CloseSquareBracket)) => start + 2,
        (OSC_8BIT, _) => start + 1,
        _ => return None,
    };
    match_lazy_dot_until(str, body, |j| match (str.get(j), str.get(j + 1)) {
        (Some(&ESC), Some(&cc::Backslash)) => Some(2),
        (Some(&BEL | &ST_8BIT), _) => Some(1),
        _ => None,
    })
}

/// `ESC_SEQUENCE`: `\x1b(?:[ #%\(\)\*\+\-\.\/]?[a-zA-Z0-9\|}~@])`.
fn match_esc_sequence(str: &[u16], start: usize) -> Option<usize> {
    if str[start] != ESC {
        return None;
    }
    let mut i = start + 1;
    if unit_in(str.get(i), b" #%()*+-./") {
        i += 1;
    }
    let is_final = is_ascii_letter(str.get(i)) || str.get(i).is_some_and(|&u| is_ascii_digit(u)) || unit_in(str.get(i), b"|}~@");
    is_final.then_some(i + 1)
}

/// `CONTROL_SEQUENCES`: o fim da sequência de controle que começa em `start`, se houver.
fn match_control_sequence(str: &[u16], start: usize) -> Option<usize> {
    match_csi_sequence(str, start)
        .or_else(|| match_osc_sequence(str, start))
        .or_else(|| match_esc_sequence(str, start))
}

/// Trechos `[início, fim)` de `str` casados por `find`, em ordem e sem sobreposição (o `matchAll` de
/// uma regex global).
fn find_all(str: &[u16], find: impl Fn(&[u16], usize) -> Option<usize>) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    let mut i = 0;
    while i < str.len() {
        match find(str, i) {
            Some(end) => {
                matches.push((i, end));
                i = end;
            }
            None => i += 1,
        }
    }
    matches
}

/// Um trecho devolvido por [`for_ansi_string_parts`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnsiStringPart<'a> {
    pub is_code: bool,
    pub str: &'a [u16],
}

/// `forAnsiStringParts`: parte o texto em trechos de texto puro e sequências de controle.
pub fn for_ansi_string_parts(str: &[u16]) -> Vec<AnsiStringPart<'_>> {
    let mut parts = Vec::new();
    let mut last = 0;
    for (start, end) in find_all(str, match_control_sequence) {
        if last != start {
            parts.push(AnsiStringPart { is_code: false, str: &str[last..start] });
        }
        parts.push(AnsiStringPart { is_code: true, str: &str[start..end] });
        last = end;
    }
    if last != str.len() {
        parts.push(AnsiStringPart { is_code: false, str: &str[last..] });
    }
    parts
}

/// Copia `str` sem os trechos `[início, fim)` de `removed`.
fn without_matches(str: &[u16], removed: &[(usize, usize)]) -> JsString {
    let mut out = JsString::new();
    let mut last = 0;
    for &(start, end) in removed {
        out.push_units(&str[last..start]);
        last = end;
    }
    out.push_units(&str[last..]);
    out
}

/// `removeAnsiEscapeCodes`: tira as sequências de escape ANSI do texto.
pub fn remove_ansi_escape_codes(str: &[u16]) -> JsString {
    without_matches(str, &find_all(str, match_control_sequence))
}

/// `PROMPT_NON_PRINTABLE`: `\\\[.*?\\\]`, ou seja, `\[` até o primeiro `\]` na mesma linha.
fn match_prompt_non_printable(str: &[u16], start: usize) -> Option<usize> {
    if str[start] != cc::Backslash || str.get(start + 1) != Some(&cc::OpenSquareBracket) {
        return None;
    }
    match_lazy_dot_until(str, start + 2, |j| {
        (str.get(j) == Some(&cc::Backslash) && str.get(j + 1) == Some(&cc::CloseSquareBracket)).then_some(2)
    })
}

/// `removeAnsiEscapeCodesFromPrompt`: tira as sequências de escape de um prompt estilo UNIX (`$PS1`).
pub fn remove_ansi_escape_codes_from_prompt(str: &[u16]) -> JsString {
    let without_codes = remove_ansi_escape_codes(str);
    without_matches(without_codes.units(), &find_all(without_codes.units(), match_prompt_non_printable))
}

/// `startsWithUTF8BOM`.
pub fn starts_with_utf8_bom(str: &[u16]) -> bool {
    str.first() == Some(&cc::UTF8_BOM)
}

/// `stripUTF8BOM`.
pub fn strip_utf8_bom(str: &[u16]) -> &[u16] {
    if starts_with_utf8_bom(str) { &str[1..] } else { str }
}

/// `fuzzyContains`: se os caracteres de `query` aparecem em `target`, na ordem, sem precisar ser
/// contíguos.
pub fn fuzzy_contains(target: &[u16], query: &[u16]) -> bool {
    if target.is_empty() || query.is_empty() {
        return false; // retorno antecipado se `target` ou `query` não existirem
    }
    if target.len() < query.len() {
        return false; // impossível `query` estar contida em `target`
    }

    let target_lower = JsString::from(target).to_lower_case();
    let query_lower = JsString::from(query).to_lower_case();
    // `toLowerCase()` pode mudar o tamanho (`\u{130}` vira `i\u{307}`), então itera sobre a consulta
    // já em minúsculas.
    let mut last_index_of: i64 = -1;
    for &unit in query_lower.units() {
        match index_of(target_lower.units(), &[unit], (last_index_of + 1) as usize) {
            Some(index) => last_index_of = index as i64,
            None => return false,
        }
    }
    true
}

/// `containsUppercaseCharacter`: com `ignore_escaped_chars`, o `\` e o caractere seguinte não contam.
pub fn contains_uppercase_character(target: &[u16], ignore_escaped_chars: bool) -> bool {
    if target.is_empty() {
        return false;
    }
    let target = if ignore_escaped_chars {
        // `/\\./g`: o `.` não casa com terminador de linha.
        let escaped = find_all(target, |str, i| {
            (str[i] == cc::Backslash && str.get(i + 1).is_some_and(|&u| !is_line_terminator(u))).then_some(i + 2)
        });
        without_matches(target, &escaped)
    } else {
        JsString::from(target)
    };
    target.to_lower_case() != target
}

/// `uppercaseFirstLetter`.
pub fn uppercase_first_letter(str: &[u16]) -> JsString {
    let first = substring(str, 0, 1);
    let mut out = JsString::from(first).to_upper_case();
    out.push_units(substring(str, 1, str.len() as i64));
    out
}

/// `getNLines`: as `n` primeiras linhas de `str`, sem a última quebra.
pub fn get_n_lines(str: &[u16], n: usize) -> &[u16] {
    if n == 0 {
        return &[];
    }
    let mut n = n;
    let mut idx: Option<usize> = None;
    loop {
        idx = index_of(str, &[cc::LineFeed], idx.map_or(0, |i| i + 1));
        n -= 1;
        if n == 0 || idx.is_none() {
            break;
        }
    }
    let Some(mut idx) = idx else {
        return str;
    };
    if idx > 0 && str[idx - 1] == cc::CarriageReturn {
        idx -= 1;
    }
    &str[..idx]
}

/// `singleLetterHash`: produz 'a'-'z', depois 'A'-'Z', e volta a 'a'-'z'.
pub fn single_letter_hash(n: usize) -> JsString {
    let letters_count = usize::from(cc::Z - cc::A + 1);
    let n = n % (2 * letters_count);
    let code = if n < letters_count { usize::from(cc::a) + n } else { usize::from(cc::A) + n - letters_count };
    JsString::from_units(vec![code as u16])
}

//#region Unicode Grapheme Break

/// `GraphemeBreakType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphemeBreakType {
    Other = 0,
    Prepend = 1,
    CR = 2,
    LF = 3,
    Control = 4,
    Extend = 5,
    RegionalIndicator = 6,
    SpacingMark = 7,
    L = 8,
    V = 9,
    T = 10,
    LV = 11,
    LVT = 12,
    ZWJ = 13,
    ExtendedPictographic = 14,
}

impl GraphemeBreakType {
    fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::Prepend,
            2 => Self::CR,
            3 => Self::LF,
            4 => Self::Control,
            5 => Self::Extend,
            6 => Self::RegionalIndicator,
            7 => Self::SpacingMark,
            8 => Self::L,
            9 => Self::V,
            10 => Self::T,
            11 => Self::LV,
            12 => Self::LVT,
            13 => Self::ZWJ,
            14 => Self::ExtendedPictographic,
            _ => Self::Other,
        }
    }
}

/// `getGraphemeBreakType`: busca o tipo na árvore binária de intervalos (`GraphemeBreakTree`).
pub fn get_grapheme_break_type(code_point: u32) -> GraphemeBreakType {
    // ASCII de 7 bits mais rápido: 0..31.
    if code_point < 32 {
        return match code_point {
            0x0A => GraphemeBreakType::LF,
            0x0D => GraphemeBreakType::CR,
            _ => GraphemeBreakType::Control,
        };
    }
    // ASCII de 7 bits mais rápido: 32..126.
    if code_point < 127 {
        return GraphemeBreakType::Other;
    }

    let data = &tables::GRAPHEME_BREAK_RAW_DATA;
    if code_point == NAN_CODE_POINT {
        // Com `NaN` nenhuma comparação é verdadeira: a busca "acerta" já na raiz.
        return GraphemeBreakType::from_raw(data[5]);
    }
    let node_count = data.len() / 3;
    let mut node_index = 1;
    while node_index <= node_count {
        let (first, last, kind) = match data.get(3 * node_index..3 * node_index + 3) {
            Some(&[first, last, kind]) => (first, last, kind),
            _ => break,
        };
        if code_point < first {
            node_index *= 2; // esquerda
        } else if code_point > last {
            node_index = 2 * node_index + 1; // direita
        } else {
            return GraphemeBreakType::from_raw(kind);
        }
    }
    GraphemeBreakType::Other
}

fn break_between_grapheme_break_type(break_type_a: GraphemeBreakType, break_type_b: GraphemeBreakType) -> bool {
    use GraphemeBreakType::*;
    // http://www.unicode.org/reports/tr29/#Grapheme_Cluster_Boundary_Rules

    // O caso comum, mais rápido. Ver a tabela em
    // https://www.unicode.org/Public/13.0.0/ucd/auxiliary/GraphemeBreakTest-13.0.0d10.html#table
    if break_type_a == Other {
        return break_type_b != Extend && break_type_b != SpacingMark;
    }

    // GB3: CR × LF. GB4 e GB5: quebra antes e depois de controles.
    if break_type_a == CR && break_type_b == LF {
        return false; // GB3
    }
    if matches!(break_type_a, Control | CR | LF) {
        return true; // GB4
    }
    if matches!(break_type_b, Control | CR | LF) {
        return true; // GB5
    }

    // Não quebra sequências de sílabas hangul.
    // GB6: L × (L | V | LV | LVT). GB7: (LV | V) × (V | T). GB8: (LVT | T) × T.
    if break_type_a == L && matches!(break_type_b, L | V | LV | LVT) {
        return false; // GB6
    }
    if matches!(break_type_a, LV | V) && matches!(break_type_b, V | T) {
        return false; // GB7
    }
    if matches!(break_type_a, LVT | T) && break_type_b == T {
        return false; // GB8
    }

    // GB9: × (Extend | ZWJ).
    if matches!(break_type_b, Extend | ZWJ) {
        return false; // GB9
    }

    // GB9a: × SpacingMark. GB9b: Prepend ×.
    if break_type_b == SpacingMark {
        return false; // GB9a
    }
    if break_type_a == Prepend {
        return false; // GB9b
    }

    // GB11: \p{Extended_Pictographic} Extend* ZWJ × \p{Extended_Pictographic}. A regra não é
    // implementada por inteiro, para não introduzir estados.
    if break_type_a == ZWJ && break_type_b == ExtendedPictographic {
        return false; // GB11
    }

    // GB12 e GB13: (RI RI)* RI × RI. Também sem estado.
    if break_type_a == RegionalIndicator && break_type_b == RegionalIndicator {
        return false; // GB12 e GB13
    }

    // GB999: Any ÷ Any.
    true
}

//#endregion

/// `getLeftDeleteOffset`: o deslocamento depois de apagar à esquerda, considerando as regras de
/// grafema e emoji do Unicode.
pub fn get_left_delete_offset(offset: usize, str: &[u16]) -> usize {
    if offset == 0 {
        return 0;
    }

    // Tenta apagar uma parte de emoji.
    if let Some(emoji_offset) = get_offset_before_last_emoji_component(offset, str) {
        return emoji_offset;
    }

    // Senão, pula um code point.
    let mut iterator = CodePointIterator::new(str, offset);
    iterator.prev_code_point();
    iterator.offset() as usize
}

fn get_offset_before_last_emoji_component(initial_offset: usize, str: &[u16]) -> Option<usize> {
    // Estrutura dos emojis: https://www.unicode.org/reports/tr51/tr51-14.html#EBNF_and_Regex
    let mut iterator = CodePointIterator::new(str, initial_offset);
    let mut code_point = iterator.prev_code_point();

    // Pula os modificadores.
    while is_emoji_modifier(code_point)
        || code_point == code_point::EMOJI_VARIANT_SELECTOR
        || code_point == code_point::ENCLOSING_KEY_CAP
    {
        if iterator.offset() == 0 {
            // Não dá para pular o modificador: não há emoji base antes.
            return None;
        }
        code_point = iterator.prev_code_point();
    }

    // Espera o emoji base.
    if !is_emoji_imprecise(code_point) {
        // Code point inesperado, não é um emoji válido.
        return None;
    }

    let mut result_offset = iterator.offset();

    if result_offset > 0 {
        // Pula os ZWJ opcionais que combinam vários emojis. Em tese, deveria checar se o ZWJ combina
        // mesmo vários emojis, para não apagar ZWJ em situações não previstas.
        let optional_zwj_code_point = iterator.prev_code_point();
        if optional_zwj_code_point == code_point::ZWJ {
            result_offset = iterator.offset();
        }
    }

    Some(result_offset as usize)
}

fn is_emoji_modifier(code_point: u32) -> bool {
    (0x1F3FB..=0x1F3FF).contains(&code_point)
}

/// Pares (code point, ASCII) achatados do dado gerado, como mapa.
fn array_to_map(pairs: &[u32]) -> HashMap<u32, u32> {
    pairs.as_chunks::<2>().0.iter().map(|&[key, value]| (key, value)).collect()
}

fn ambiguous_data(locale: &str) -> Option<&'static [u32]> {
    tables::AMBIGUOUS_CHARACTER_DATA.iter().find(|(key, _)| *key == locale).map(|&(_, values)| values)
}

/// `AmbiguousCharacters`: caracteres que se confundem com ASCII, por locale.
pub struct AmbiguousCharacters {
    confusable_dictionary: HashMap<u32, u32>,
}

/// O cache de uma entrada do `LRUCachedFunction` do original, com a lista de locales como chave.
static AMBIGUOUS_CACHE: Mutex<Option<(String, Arc<AmbiguousCharacters>)>> = Mutex::new(None);

impl AmbiguousCharacters {
    /// `AmbiguousCharacters.getInstance`.
    pub fn get_instance<S: AsRef<str>>(locales: impl IntoIterator<Item = S>) -> Arc<AmbiguousCharacters> {
        let locales_str = locales.into_iter().map(|l| l.as_ref().to_owned()).collect::<Vec<_>>().join(",");
        let mut cache = AMBIGUOUS_CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((key, instance)) = cache.as_ref()
            && *key == locales_str
        {
            return Arc::clone(instance);
        }
        let instance = Arc::new(Self::compute(&locales_str));
        *cache = Some((locales_str, Arc::clone(&instance)));
        instance
    }

    fn compute(locales_str: &str) -> Self {
        let mut filtered_locales: Vec<&str> =
            locales_str.split(',').filter(|l| !l.starts_with('_') && ambiguous_data(l).is_some()).collect();
        if filtered_locales.is_empty() {
            filtered_locales = vec!["_default"];
        }

        let mut language_specific_map: Option<HashMap<u32, u32>> = None;
        for locale in filtered_locales {
            let map = array_to_map(ambiguous_data(locale).unwrap_or_default());
            language_specific_map = Some(match language_specific_map {
                None => map,
                Some(previous) => previous.into_iter().filter(|(key, _)| map.contains_key(key)).collect(),
            });
        }

        let mut confusable_dictionary = array_to_map(ambiguous_data("_common").unwrap_or_default());
        confusable_dictionary.extend(language_specific_map.unwrap_or_default());
        Self { confusable_dictionary }
    }

    /// `AmbiguousCharacters.getLocales`.
    pub fn get_locales() -> Vec<&'static str> {
        tables::AMBIGUOUS_CHARACTER_DATA.iter().map(|&(key, _)| key).filter(|k| !k.starts_with('_')).collect()
    }

    pub fn is_ambiguous(&self, code_point: u32) -> bool {
        self.confusable_dictionary.contains_key(&code_point)
    }

    pub fn contains_ambiguous_character(&self, str: &[u16]) -> bool {
        (0..str.len()).any(|i| code_point_at(str, i).is_some_and(|code_point| self.is_ambiguous(code_point)))
    }

    /// O code point ASCII com que o dado pode ser confundido, se existir.
    pub fn get_primary_confusable(&self, code_point: u32) -> Option<u32> {
        self.confusable_dictionary.get(&code_point).copied()
    }

    pub fn get_confusable_code_points(&self) -> HashSet<u32> {
        self.confusable_dictionary.keys().copied().collect()
    }
}

/// `InvisibleCharacters`.
pub struct InvisibleCharacters;

impl InvisibleCharacters {
    fn get_data() -> &'static HashSet<u32> {
        static DATA: OnceLock<HashSet<u32>> = OnceLock::new();
        DATA.get_or_init(|| tables::INVISIBLE_CHARACTER_DATA.iter().flat_map(|&(_, values)| values.iter().copied()).collect())
    }

    pub fn is_invisible_character(code_point: u32) -> bool {
        Self::get_data().contains(&code_point)
    }

    /// O espaço comum também conta.
    pub fn contains_invisible_character(str: &[u16]) -> bool {
        (0..str.len()).any(|i| {
            code_point_at(str, i)
                .is_some_and(|code_point| Self::is_invisible_character(code_point) || code_point == code_point::SPACE)
        })
    }

    pub fn code_points() -> &'static HashSet<u32> {
        Self::get_data()
    }
}

/// `toBinary`: cada unidade UTF-16 vira dois bytes (ordem do `Uint16Array` da plataforma, little-endian).
fn to_binary(str: &[u16]) -> Vec<u8> {
    str.iter().flat_map(|unit| unit.to_le_bytes()).collect()
}

/// `multibyteAwareBtoa`: como o `btoa` global, mas aceita caracteres multibyte em vez de lançar
/// exceção (codifica os bytes das unidades UTF-16 em base64).
pub fn multibyte_aware_btoa(str: &[u16]) -> JsString {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for chunk in to_binary(str).chunks(3) {
        let group = chunk.iter().enumerate().fold(0u32, |acc, (i, &byte)| acc | u32::from(byte) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(u16::from(ALPHABET[(group >> (18 - 6 * i) & 0x3F) as usize]));
            } else {
                out.push(u16::from(b'='));
            }
        }
    }
    JsString::from_units(out)
}
