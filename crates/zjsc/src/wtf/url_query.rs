//! Porte de `WTF/wtf/URLParser.cpp` das peças que os estados `UTF8Query`, `NonUTF8Query` e `Fragment`
//! usam: `shouldPercentEncodeQueryByte`, `findQueryStopCharacter`, `findFragmentStopCharacter`,
//! `appendCodePoint`, `URLParser::utf8QueryEncode`, `URLParser::encodeNonUTF8Query`, `URLTextEncoding`
//! e o corpo dos três estados.
//!
//! O estado do `URLParser` (buffer ASCII, `m_didSeeSyntaxViolation`, `m_urlIsSpecial`) é privado de
//! `url_parser.rs`; por isso as funções livres daqui falam com o parser pelo trait [`QuerySink`], que o
//! `URLParser` implementa com os métodos que ele já tem.
//!
//! O fallback `&#NNN;` para caractere não representável na codificação NÃO existe em `WTF`: o upstream
//! `URLTextEncoding` é só a interface com `encodeForURLParsing(StringView)` (URL.h 46 a 52). Quem
//! implementa a codificação concreta (WebCore `TextEncoding`) fica fora desta árvore, então nada
//! disso foi inventado aqui.

use crate::wtf::url_character_class_table::{CHARACTER_CLASS_TABLE, QUERY_ENCODE};
use crate::wtf::url_parser::CodePointIterator;

/// `class URLTextEncoding` (URL.h 46): `virtual Vector<uint8_t> encodeForURLParsing(StringView) const = 0`.
pub trait UrlTextEncoding {
    fn encode_for_url_parsing(&self, source: &[u16]) -> Vec<u8>;
}

/// O ponteiro `const URLTextEncoding*` do C++ com o sentinela `URLTextEncodingSentinelAllowingC0AtEnd`
/// (`reinterpret_cast<const URLTextEncoding*>(-1)`, URLParser.h 51): nulo, sentinela ou codificação.
#[derive(Clone, Copy)]
pub enum QueryEncoding<'a> {
    /// `nullptr`.
    None,
    /// `URLTextEncodingSentinelAllowingC0AtEnd`: `parse` o troca por `nullptr` sem cortar C0/espaço do fim.
    SentinelAllowingC0AtEnd,
    /// Codificação não UTF-8 da query.
    Encoding(&'a dyn UrlTextEncoding),
}

/// O que o `URLParser` oferece às funções deste módulo. Cada método é o homônimo do C++.
pub trait QuerySink {
    /// `m_urlIsSpecial`.
    fn url_is_special(&self) -> bool;
    /// `URLParser::syntaxViolation(const CodePointIterator&)` (URLParser.cpp 1432).
    fn syntax_violation(&mut self, input: &[u16], iterator: &CodePointIterator);
    /// `URLParser::percentEncodeByte(uint8_t)` (URLParser.cpp 899).
    fn percent_encode_byte(&mut self, byte: u8);
    /// `URLParser::appendToASCIIBuffer(char32_t)` (URLParser.cpp 776).
    fn append_to_ascii_buffer(&mut self, code_point: u32);
    /// `URLParser::appendToASCIIBuffer(std::span<...>)` (URLParser.cpp 783 e 789).
    fn append_span_to_ascii_buffer(&mut self, characters: &[u16]);
    /// `URLParser::utf8PercentEncode<isInCodeSet>` (URLParser.cpp 908).
    fn utf8_percent_encode(&mut self, input: &[u16], iterator: &CodePointIterator, is_in_code_set: fn(u32) -> bool);
    /// `URLParser::advance(iterator, iteratorForSyntaxViolationPosition)` com `ReportSyntaxViolation::Yes`
    /// (URLParser.cpp 739).
    fn advance_for(&mut self, input: &[u16], iterator: &mut CodePointIterator, for_syntax_violation: &CodePointIterator);
}

/// `isTabOrNewline` (URLParser.cpp 331).
fn is_tab_or_newline(character: u32) -> bool {
    character == 0x09 || character == 0x0A || character == 0x0D
}

/// `shouldPercentEncodeQueryByte` (URLParser.cpp 719).
pub fn should_percent_encode_query_byte(byte: u8, url_is_special: bool) -> bool {
    if CHARACTER_CLASS_TABLE[byte as usize] & QUERY_ENCODE != 0 {
        return true;
    }
    byte == b'\'' && url_is_special
}

/// Bit `QueryStop` de `scanClassTable` (URLParser.cpp 365): `c > 0x7E || (table[c] & QueryEncode) || c == '\'' || c == '#'`.
fn is_query_stop(unit: u16) -> bool {
    unit > 0x7E
        || CHARACTER_CLASS_TABLE[unit as usize] & QUERY_ENCODE != 0
        || unit == b'\'' as u16
        || unit == b'#' as u16
}

/// Bit `FragmentStop` de `scanClassTable` (URLParser.cpp 368): `c > 0x7E || c == '`' || ((table[c] & QueryEncode) && c != '#')`.
fn is_fragment_stop(unit: u16) -> bool {
    unit > 0x7E
        || unit == b'`' as u16
        || (CHARACTER_CLASS_TABLE[unit as usize] & QUERY_ENCODE != 0 && unit != b'#' as u16)
}

/// `findQueryStopCharacter` (URLParser.cpp 650): primeiro índice a partir de `begin` com o bit `QueryStop`,
/// ou `data.len()`. A versão SIMD e a de tabela devolvem o mesmo ponto.
pub fn find_query_stop_character(data: &[u16], begin: usize) -> usize {
    data[begin..].iter().position(|&unit| is_query_stop(unit)).map_or(data.len(), |offset| begin + offset)
}

/// `findFragmentStopCharacter` (URLParser.cpp 658): idem com o bit `FragmentStop`.
pub fn find_fragment_stop_character(data: &[u16], begin: usize) -> usize {
    data[begin..].iter().position(|&unit| is_fragment_stop(unit)).map_or(data.len(), |offset| begin + offset)
}

/// `appendCodePoint(Vector<char16_t>&, char32_t)` (URLParser.cpp 46).
pub fn append_code_point(destination: &mut Vec<u16>, code_point: u32) {
    if code_point < 0x10000 {
        destination.push(code_point as u16);
        return;
    }
    let value = code_point - 0x10000;
    destination.push(0xD800 + (value >> 10) as u16);
    destination.push(0xDC00 + (value & 0x3FF) as u16);
}

/// `URLParser::utf8QueryEncode` (URLParser.cpp 937).
pub fn utf8_query_encode<S: QuerySink>(sink: &mut S, input: &[u16], iterator: &CodePointIterator) {
    debug_assert!(!iterator.at_end());
    let code_point = iterator.get();
    if code_point < 0x80 {
        if should_percent_encode_query_byte(code_point as u8, sink.url_is_special()) {
            sink.syntax_violation(input, iterator);
            sink.percent_encode_byte(code_point as u8);
        } else {
            sink.append_to_ascii_buffer(code_point);
        }
        return;
    }

    sink.syntax_violation(input, iterator);

    // U8_APPEND falha para substituto órfão: o C++ anexa replacementCharacterUTF8PercentEncoded ("%EF%BF%BD").
    match char::from_u32(code_point) {
        Some(character) => {
            let mut buffer = [0u8; 4];
            for &byte in character.encode_utf8(&mut buffer).as_bytes() {
                if should_percent_encode_query_byte(byte, sink.url_is_special()) {
                    sink.percent_encode_byte(byte);
                } else {
                    sink.append_to_ascii_buffer(byte as u32);
                }
            }
        }
        None => {
            let replacement: Vec<u16> = b"%EF%BF%BD".iter().map(|&byte| byte as u16).collect();
            sink.append_span_to_ascii_buffer(&replacement);
        }
    }
}

/// `URLParser::encodeNonUTF8Query` (URLParser.cpp 969). `iterator` cobre de `queryBegin` até `c`
/// (`CodePointIterator<CharacterType>(queryBegin, c)`): `CodePointIterator::new(&input[..c.position()], query_begin.position())`.
pub fn encode_non_utf8_query<S: QuerySink>(
    sink: &mut S,
    input: &[u16],
    source: &[u16],
    encoding: &dyn UrlTextEncoding,
    mut iterator: CodePointIterator,
) {
    let encoded = encoding.encode_for_url_parsing(source);
    let length = encoded.len();

    if (length == 0) == iterator.at_end() {
        sink.syntax_violation(input, &iterator);
        return;
    }

    let mut i = 0;
    while i < length {
        debug_assert!(!iterator.at_end());
        let byte = encoded[i];
        if byte as u32 != iterator.get() {
            sink.syntax_violation(input, &iterator);
            break;
        }
        if should_percent_encode_query_byte(byte, sink.url_is_special()) {
            sink.syntax_violation(input, &iterator);
            break;
        }
        sink.append_to_ascii_buffer(byte as u32);
        iterator.advance_unit();
        i += 1;
    }
    while !iterator.at_end() && is_tab_or_newline(iterator.get()) {
        iterator.advance_unit();
    }
    debug_assert_eq!(i == length, iterator.at_end());
    while i < length {
        let byte = encoded[i];
        if should_percent_encode_query_byte(byte, sink.url_is_special()) {
            sink.percent_encode_byte(byte);
        } else {
            sink.append_to_ascii_buffer(byte as u32);
        }
        i += 1;
    }
}

/// O que um passo de estado de query pede ao laço de `parse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryStateStep {
    /// Continua no mesmo estado (o `break` do `case`, sem trocar `state`).
    Stay,
    /// `m_url.m_queryEnd = currentPosition(c); state = State::Fragment;` (quem chama grava `m_queryEnd`).
    ToFragment,
}

/// Corpo de `case State::UTF8Query` (URLParser.cpp 2635). `input` é a entrada cortada em `endIndex`,
/// a mesma de `c`.
pub fn utf8_query_state<'a, S: QuerySink>(sink: &mut S, input: &'a [u16], c: &mut CodePointIterator<'a>) -> QueryStateStep {
    let start = c.position();
    let p = find_query_stop_character(input, start);
    sink.append_span_to_ascii_buffer(&input[start..p]);
    *c = CodePointIterator::new(input, p);
    if c.at_end() || is_tab_or_newline(c.get()) {
        return QueryStateStep::Stay;
    }
    if c.get() == '#' as u32 {
        return QueryStateStep::ToFragment;
    }
    utf8_query_encode(sink, input, c);
    c.advance_unit();
    QueryStateStep::Stay
}

/// Corpo de `case State::NonUTF8Query` (URLParser.cpp 2652): o `do { ... } while (!c.atEnd())`.
/// `query_buffer` é o `Vector<char16_t> queryBuffer` de `parse`; `query_begin` o `queryBegin`.
/// Em `#`, codifica a query acumulada e devolve `ToFragment` (quem chama grava `m_queryEnd`).
pub fn non_utf8_query_state<S: QuerySink>(
    sink: &mut S,
    input: &[u16],
    c: &mut CodePointIterator,
    query_begin: &CodePointIterator,
    query_buffer: &mut Vec<u16>,
    encoding: &dyn UrlTextEncoding,
) -> QueryStateStep {
    loop {
        if c.get() == '#' as u32 {
            let span = CodePointIterator::new(&input[..c.position()], query_begin.position());
            encode_non_utf8_query(sink, input, query_buffer, encoding, span);
            return QueryStateStep::ToFragment;
        }
        append_code_point(query_buffer, c.get());
        sink.advance_for(input, c, query_begin);
        if c.at_end() {
            return QueryStateStep::Stay;
        }
    }
}

/// Corpo de `case State::Fragment` (URLParser.cpp 2668). `is_in_fragment_encode_set` é o
/// `isInFragmentEncodeSet` (URLParser.cpp 333) que `url_parser.rs` já tem.
pub fn fragment_state<'a, S: QuerySink>(
    sink: &mut S,
    input: &'a [u16],
    c: &mut CodePointIterator<'a>,
    is_in_fragment_encode_set: fn(u32) -> bool,
) {
    let start = c.position();
    let p = find_fragment_stop_character(input, start);
    sink.append_span_to_ascii_buffer(&input[start..p]);
    *c = CodePointIterator::new(input, p);
    if c.at_end() || is_tab_or_newline(c.get()) {
        return;
    }
    sink.utf8_percent_encode(input, c, is_in_fragment_encode_set);
    c.advance_unit();
}

/// `case State::NonUTF8Query` do estado final (URLParser.cpp 2888): codifica a query pendente.
/// Quem chama grava `m_queryEnd = currentPosition(c)` depois.
pub fn finish_non_utf8_query<S: QuerySink>(
    sink: &mut S,
    input: &[u16],
    c: &CodePointIterator,
    query_begin: &CodePointIterator,
    query_buffer: &[u16],
    encoding: &dyn UrlTextEncoding,
) {
    let span = CodePointIterator::new(&input[..c.position()], query_begin.position());
    encode_non_utf8_query(sink, input, query_buffer, encoding, span);
}
