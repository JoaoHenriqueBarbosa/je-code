//! Os ajudantes de `marked.js` (`escape`, `rtrim`, `findClosingBracket`, `splitCells`...) e as operações de
//! `String` do JavaScript que o lexer usa, sobre fatias de unidades UTF-16.

use crate::JsString;
use crate::js_string::{is_js_whitespace, trim};

pub(crate) fn u(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

pub(crate) fn ch(c: char) -> u16 {
    u16::try_from(u32::from(c)).expect("caractere ASCII")
}

/// `a + b`.
pub(crate) fn concat(parts: &[&[u16]]) -> JsString {
    let mut out = JsString::new();
    for part in parts {
        out.push_units(part);
    }
    out
}

/// Um índice de `slice`/`substring` já normalizado para `0..=len`.
fn clamp_index(index: i64, len: usize) -> usize {
    if index < 0 {
        len.saturating_sub(index.unsigned_abs() as usize)
    } else {
        (index as usize).min(len)
    }
}

/// `str.slice(start, end)` (índices negativos contam do fim).
pub(crate) fn slice(units: &[u16], start: i64, end: i64) -> &[u16] {
    let (a, b) = (clamp_index(start, units.len()), clamp_index(end, units.len()));
    if a >= b { &[] } else { &units[a..b] }
}

/// `str.slice(start)`.
pub(crate) fn slice_from(units: &[u16], start: i64) -> &[u16] {
    &units[clamp_index(start, units.len())..]
}

/// `str.substring(start)` com um comprimento de `raw`: limita ao texto.
pub(crate) fn tail(units: &[u16], start: usize) -> &[u16] {
    &units[start.min(units.len())..]
}

/// `str.substring(0, end)`.
pub(crate) fn head(units: &[u16], end: usize) -> &[u16] {
    &units[..end.min(units.len())]
}

/// `str.split('\n')`.
pub(crate) fn split_lines(units: &[u16]) -> Vec<&[u16]> {
    units.split(|&c| c == 0x0A).collect()
}

/// `str.split('\n', 1)[0]`: a primeira linha.
pub(crate) fn first_line(units: &[u16]) -> &[u16] {
    units.split(|&c| c == 0x0A).next().unwrap_or_default()
}

/// `parts.join('\n')`.
pub(crate) fn join_lines(lines: &[&[u16]]) -> Vec<u16> {
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push(0x0A);
        }
        out.extend_from_slice(line);
    }
    out
}

/// `str.search(/[^ ]/)`: o índice do primeiro caractere que não é espaço, ou `-1`.
pub(crate) fn search_non_space(units: &[u16]) -> i64 {
    units.iter().position(|&c| c != 0x20).map_or(-1, |i| i as i64)
}

/// `/^ *$/.test(str)`.
pub(crate) fn only_spaces(units: &[u16]) -> bool {
    units.iter().all(|&c| c == 0x20)
}

/// O tamanho em pontos de código (`[...str].length`; um surrogate isolado conta como um).
pub(crate) fn code_point_len(units: &[u16]) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < units.len() {
        i += code_point_unit_len(&units[i..]);
        count += 1;
    }
    count
}

/// O número de unidades do primeiro ponto de código (`[...str][0].length`).
pub(crate) fn code_point_unit_len(units: &[u16]) -> usize {
    match units {
        [0xD800..=0xDBFF, 0xDC00..=0xDFFF, ..] => 2,
        [] => 0,
        _ => 1,
    }
}

const ESCAPE_REPLACEMENTS: [(char, &str); 5] = [
    ('&', "&amp;"),
    ('<', "&lt;"),
    ('>', "&gt;"),
    ('"', "&quot;"),
    ('\'', "&#39;"),
];

fn escape_unit(unit: u16) -> Option<&'static str> {
    ESCAPE_REPLACEMENTS
        .iter()
        .find(|(c, _)| ch(*c) == unit)
        .map(|(_, r)| *r)
}

/// `&` seguido de `(#\d{1,7}|#[Xx][a-fA-F0-9]{1,6}|\w+);`: uma entidade que `escape` sem `encode` preserva.
fn entity_at(units: &[u16], amp: usize) -> bool {
    let rest = &units[amp + 1..];
    let is_digit = |c: u16| (0x30..=0x39).contains(&c);
    let is_hex = |c: u16| is_digit(c) || (0x41..=0x46).contains(&c) || (0x61..=0x66).contains(&c);
    let is_word = |c: u16| is_digit(c) || (0x41..=0x5A).contains(&c) || (0x61..=0x7A).contains(&c) || c == 0x5F;
    let semicolon_at = |n: usize| rest.get(n) == Some(&0x3B);
    if rest.first() == Some(&0x23) {
        let digits = rest[1..].iter().take_while(|&&c| is_digit(c)).count();
        if (1..=7).contains(&digits) && semicolon_at(1 + digits) {
            return true;
        }
        if matches!(rest.get(1), Some(0x78 | 0x58)) {
            let hex = rest[2..].iter().take_while(|&&c| is_hex(c)).count();
            return (1..=6).contains(&hex) && semicolon_at(2 + hex);
        }
        return false;
    }
    let word = rest.iter().take_while(|&&c| is_word(c)).count();
    word >= 1 && semicolon_at(word)
}

/// `escape$1(html, encode)`.
pub(crate) fn escape(html: &[u16], encode: bool) -> JsString {
    let mut out = JsString::new();
    for (i, &c) in html.iter().enumerate() {
        match escape_unit(c) {
            Some(replacement) if c != 0x26 || encode || !entity_at(html, i) => {
                for unit in replacement.encode_utf16() {
                    out.push_unit(unit);
                }
            }
            _ => out.push_unit(c),
        }
    }
    out
}

/// `rtrim(str, c, invert)`.
pub(crate) fn rtrim(units: &[u16], c: u16, invert: bool) -> &[u16] {
    let mut suffix = 0;
    while suffix < units.len() {
        let current = units[units.len() - suffix - 1];
        if (current == c && !invert) || (current != c && invert) {
            suffix += 1;
        } else {
            break;
        }
    }
    &units[..units.len() - suffix]
}

/// `findClosingBracket(str, b)`.
pub(crate) fn find_closing_bracket(units: &[u16], open: u16, close: u16) -> i64 {
    if !units.contains(&close) {
        return -1;
    }
    let mut level = 0i64;
    let mut i = 0;
    while i < units.len() {
        if units[i] == 0x5C {
            i += 1;
        } else if units[i] == open {
            level += 1;
        } else if units[i] == close {
            level -= 1;
            if level < 0 {
                return i as i64;
            }
        }
        i += 1;
    }
    -1
}

/// `splitCells(tableRow, count)`.
pub(crate) fn split_cells(table_row: &[u16], count: Option<usize>) -> Vec<JsString> {
    let pipe = ch('|');
    let mut row: Vec<u16> = Vec::with_capacity(table_row.len() + 8);
    for (offset, &c) in table_row.iter().enumerate() {
        if c != pipe {
            row.push(c);
            continue;
        }
        let mut escaped = false;
        let mut current = offset;
        while current > 0 && table_row[current - 1] == 0x5C {
            escaped = !escaped;
            current -= 1;
        }
        if escaped {
            row.push(pipe);
        } else {
            row.push(0x20);
            row.push(pipe);
        }
    }
    let separator = [0x20, pipe];
    let mut cells: Vec<Vec<u16>> = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < row.len() {
        if row[i..].starts_with(&separator) {
            cells.push(row[start..i].to_vec());
            i += 2;
            start = i;
        } else {
            i += 1;
        }
    }
    cells.push(row[start..].to_vec());

    if trim(&cells[0]).is_empty() {
        cells.remove(0);
    }
    if cells.last().is_some_and(|last| trim(last).is_empty()) {
        cells.pop();
    }
    if let Some(count) = count.filter(|&n| n != 0) {
        if cells.len() > count {
            cells.truncate(count);
        } else {
            while cells.len() < count {
                cells.push(Vec::new());
            }
        }
    }
    cells
        .iter()
        .map(|cell| {
            let trimmed = trim(cell);
            let mut out = JsString::new();
            let mut j = 0;
            while j < trimmed.len() {
                if trimmed[j] == 0x5C && trimmed.get(j + 1) == Some(&pipe) {
                    out.push_unit(pipe);
                    j += 2;
                } else {
                    out.push_unit(trimmed[j]);
                    j += 1;
                }
            }
            out
        })
        .collect()
}

/// `indentCodeCompensation(raw, text)`.
pub(crate) fn indent_code_compensation(raw: &[u16], text: &[u16]) -> JsString {
    let indent_to_code = {
        let run = raw.iter().take_while(|&&c| is_js_whitespace(c)).count();
        let backticks = [0x60, 0x60, 0x60];
        if run == 0 || !raw[run..].starts_with(&backticks) {
            return JsString::from(text);
        }
        run
    };
    let mut out: Vec<u16> = Vec::with_capacity(text.len());
    for (i, node) in split_lines(text).into_iter().enumerate() {
        if i > 0 {
            out.push(0x0A);
        }
        let indent_in_node = node.iter().take_while(|&&c| is_js_whitespace(c)).count();
        if indent_in_node > 0 && indent_in_node >= indent_to_code {
            out.extend_from_slice(&node[indent_to_code..]);
        } else {
            out.extend_from_slice(node);
        }
    }
    JsString::from_units(out)
}

/// `str.replace(/\s+/g, ' ')`.
pub(crate) fn collapse_whitespace(units: &[u16]) -> JsString {
    let mut out = JsString::new();
    let mut in_run = false;
    for &c in units {
        if is_js_whitespace(c) {
            if !in_run {
                out.push_unit(0x20);
            }
            in_run = true;
        } else {
            out.push_unit(c);
            in_run = false;
        }
    }
    out
}
