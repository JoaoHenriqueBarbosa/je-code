//! Porte de `WTF/wtf/URLParser.{h,cpp}` e dos acessores de leitura de `WTF/wtf/URL.{h,cpp}`.
//!
//! Fatia 1: o `URL` com os offsets reais, o enum de estados, o `CodePointIterator` e o laço
//! principal até os estados `SchemeStart`, `Scheme` e `NoScheme`. Os demais estados já estão portados;
//! nada aqui
//! inventa comportamento que o C++ não tenha.
//!
//! Divergência declarada desta fatia: o C++ é um template em `Latin1Character`/`char16_t`; aqui a
//! entrada é sempre alargada para `u16` (o resultado é o mesmo, só muda o custo). O trecho
//! "straight-line pass" (URLParser.cpp 1647 a 1801) é apenas um atalho de desempenho que deixa
//! `state` e `c` exatamente como a máquina de estados os teria: ele fica fora e a máquina de
//! estados cobre a entrada inteira.

use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url_query::{self, QueryEncoding, QuerySink, QueryStateStep, UrlTextEncoding};
use crate::wtf::url_character_class_table::{
    CHARACTER_CLASS_TABLE, FORBIDDEN_DOMAIN, FORBIDDEN_HOST, PATH_ENCODE, QUERY_ENCODE, SLASH_QUESTION_OR_HASH,
    USER_INFO_ENCODE, VALID_SCHEME,
};

/// `URL::maxSchemeLength` (URL.h 281).
pub const MAX_SCHEME_LENGTH: u32 = (1 << 26) - 1;

/// `enum class Scheme` (URLParser.cpp 1062).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Ws,
    Wss,
    File,
    Ftp,
    Http,
    Https,
    NonSpecial,
}

/// `defaultPort(Scheme)` (URLParser.cpp 1073). `None` equivale ao `numeric_limits<unsigned>::max()`.
pub fn default_port(scheme: Scheme) -> Option<u32> {
    match scheme {
        Scheme::Ws | Scheme::Http => Some(80),
        Scheme::Wss | Scheme::Https => Some(443),
        Scheme::Ftp => Some(21),
        Scheme::File | Scheme::NonSpecial => None,
    }
}

/// `schemeType(const CharactersType&, size_t)` (URLParser.cpp 1091). O comprimento é `scheme.len()`.
pub fn scheme_type<T: Copy + Into<u32>>(scheme: &[T]) -> Scheme {
    let length = scheme.len();
    if length == 0 {
        return Scheme::NonSpecial;
    }
    let at = |i: usize| -> u32 { scheme[i].into() };
    let is = |i: usize, ch: char| at(i) == ch as u32;
    match at(0) {
        0x66 /* f */ => match length {
            3 if is(1, 't') && is(2, 'p') => Scheme::Ftp,
            4 if is(1, 'i') && is(2, 'l') && is(3, 'e') => Scheme::File,
            _ => Scheme::NonSpecial,
        },
        0x68 /* h */ => match length {
            4 if is(1, 't') && is(2, 't') && is(3, 'p') => Scheme::Http,
            5 if is(1, 't') && is(2, 't') && is(3, 'p') && is(4, 's') => Scheme::Https,
            _ => Scheme::NonSpecial,
        },
        0x77 /* w */ => match length {
            2 if is(1, 's') => Scheme::Ws,
            3 if is(1, 's') && is(2, 's') => Scheme::Wss,
            _ => Scheme::NonSpecial,
        },
        _ => Scheme::NonSpecial,
    }
}

/// `URLParser::isSpecialScheme(StringView)` (URLParser.cpp 1179).
pub fn is_special_scheme<T: Copy + Into<u32>>(scheme: &[T]) -> bool {
    scheme_type(scheme) != Scheme::NonSpecial
}

/// `isC0ControlOrSpace` (URLParser.cpp 330).
fn is_c0_control_or_space(character: u32) -> bool {
    character <= 0x20
}

/// `isTabOrNewline` (URLParser.cpp 331).
fn is_tab_or_newline(character: u32) -> bool {
    character == 0x09 || character == 0x0A || character == 0x0D
}

fn is_ascii_alpha(character: u32) -> bool {
    (character | 0x20) >= 'a' as u32 && (character | 0x20) <= 'z' as u32
}

fn is_ascii_upper(character: u32) -> bool {
    character >= 'A' as u32 && character <= 'Z' as u32
}

fn to_ascii_lower(character: u32) -> u32 {
    if is_ascii_upper(character) { character | 0x20 } else { character }
}

/// `isForbiddenHostCodePoint(char16_t)` (URLParser.cpp 324).
pub fn is_forbidden_host_code_point(character: u16) -> bool {
    character <= 0x7F && CHARACTER_CLASS_TABLE[character as usize] & FORBIDDEN_HOST != 0
}

/// `URLParser::isForbiddenDomainCodePoint` (URLParser.cpp 714).
pub fn is_forbidden_domain_code_point(character: u32) -> bool {
    character <= 0x7F && CHARACTER_CLASS_TABLE[character as usize] & FORBIDDEN_DOMAIN != 0
}

/// `isC0Control` (URLParser.cpp 329).
#[allow(dead_code)]
fn is_c0_control(character: u32) -> bool {
    character <= 0x1F
}

/// `isInC0ControlEncodeSet` (URLParser.cpp 332).
#[allow(dead_code)]
fn is_in_c0_control_encode_set(character: u32) -> bool {
    character > 0x7E || is_c0_control(character)
}

/// `isInFragmentEncodeSet` (URLParser.cpp 333).
pub fn is_in_fragment_encode_set(character: u32) -> bool {
    character > 0x7E
        || character == '`' as u32
        || (CHARACTER_CLASS_TABLE[character as usize] & QUERY_ENCODE != 0 && character != '#' as u32)
}

/// `isInPathEncodeSet` (URLParser.cpp 334).
#[allow(dead_code)]
fn is_in_path_encode_set(character: u32) -> bool {
    character > 0x7E || CHARACTER_CLASS_TABLE[character as usize] & PATH_ENCODE != 0
}

/// `isInUserInfoEncodeSet` (URLParser.cpp 335).
pub(crate) fn is_in_user_info_encode_set(character: u32) -> bool {
    character > 0x7E || CHARACTER_CLASS_TABLE[character as usize] & USER_INFO_ENCODE != 0
}

/// `isPercentOrNonASCII` (URLParser.cpp 336).
fn is_percent_or_non_ascii(character: u32) -> bool {
    character >= 0x80 || character == '%' as u32
}

/// `isSlashQuestionOrHash` (URLParser.cpp 337).
#[allow(dead_code)]
fn is_slash_question_or_hash(character: u32) -> bool {
    character <= '\\' as u32 && CHARACTER_CLASS_TABLE[character as usize] & SLASH_QUESTION_OR_HASH != 0
}

/// `isValidSchemeCharacter` (URLParser.cpp 338).
fn is_valid_scheme_character(character: u32) -> bool {
    character <= 'z' as u32 && CHARACTER_CLASS_TABLE[character as usize] & VALID_SCHEME != 0
}

/// `advance<CharacterType, ReportSyntaxViolation::No>` (URLParser.cpp 736): avança uma unidade e pula
/// tabs e quebras de linha sem registrar violação de sintaxe.
fn advance_without_violation(iterator: &mut CodePointIterator) {
    iterator.advance_unit();
    while !iterator.at_end() && is_tab_or_newline(iterator.get()) {
        iterator.advance_unit();
    }
}

/// `isASCIIAlphaCaselessEqual(c, expectedASCIILowercaseLetter)` (ASCIICType.h 78).
fn is_ascii_alpha_caseless_equal(character: u32, expected_lowercase: char) -> bool {
    (character | 0x20) == expected_lowercase as u32
}

/// `isWindowsDriveLetter(CodePointIterator)` (URLParser.cpp 757).
fn is_windows_drive_letter(mut iterator: CodePointIterator) -> bool {
    if iterator.at_end() || !is_ascii_alpha(iterator.get()) {
        return false;
    }
    advance_without_violation(&mut iterator);
    if iterator.at_end() {
        return false;
    }
    if iterator.get() != ':' as u32 && iterator.get() != '|' as u32 {
        return false;
    }
    advance_without_violation(&mut iterator);
    iterator.at_end()
        || iterator.get() == '/' as u32
        || iterator.get() == '\\' as u32
        || iterator.get() == '?' as u32
        || iterator.get() == '#' as u32
}

/// `URLParser::takesTwoAdvancesUntilEnd` (URLParser.cpp 750): o iterador tem exatamente dois pontos de código.
fn takes_two_advances_until_end(mut iterator: CodePointIterator) -> bool {
    if iterator.at_end() {
        return false;
    }
    advance_without_violation(&mut iterator);
    if iterator.at_end() {
        return false;
    }
    advance_without_violation(&mut iterator);
    iterator.at_end()
}

/// `URLParser::checkLocalhostCodePoint` (URLParser.cpp 1462).
fn check_localhost_code_point(iterator: &mut CodePointIterator, code_point: char) -> bool {
    if iterator.at_end() || to_ascii_lower(iterator.get()) != code_point as u32 {
        return false;
    }
    advance_without_violation(iterator);
    true
}

/// `URLParser::isAtLocalhost` (URLParser.cpp 1471) e `isLocalhost(StringView)` (1494): `localhost` sem caixa.
fn is_localhost(view: &[u16]) -> bool {
    let mut iterator = CodePointIterator::new(view, 0);
    "localhost".chars().all(|expected| check_localhost_code_point(&mut iterator, expected)) && iterator.at_end()
}

/// Os bytes ASCII como code units, para `appendToASCIIBuffer("..."_span8)`.
fn ascii_units<const N: usize>(text: &[u8; N]) -> [u16; N] {
    text.map(|byte| byte as u16)
}

/// `isSingleDotPathSegment` (URLParser.cpp 1310):`.` ou `%2e` (sem caixa) seguido de fim, `/`, `\`, `?` ou `#`.
fn is_single_dot_path_segment(mut c: CodePointIterator) -> bool {
    if c.at_end() {
        return false;
    }
    if c.get() == '.' as u32 {
        advance_without_violation(&mut c);
        return c.at_end() || is_slash_question_or_hash(c.get());
    }
    if c.get() != '%' as u32 {
        return false;
    }
    advance_without_violation(&mut c);
    // dotASCIICode { '2', 'e' } (URLParser.cpp 1307).
    if c.at_end() || c.get() != '2' as u32 {
        return false;
    }
    advance_without_violation(&mut c);
    if c.at_end() {
        return false;
    }
    if is_ascii_alpha_caseless_equal(c.get(), 'e') {
        advance_without_violation(&mut c);
        return c.at_end() || is_slash_question_or_hash(c.get());
    }
    false
}

/// `isDoubleDotPathSegment` (URLParser.cpp 1334): dois pontos (cada um `.` ou `%2e`) formando um segmento.
fn is_double_dot_path_segment(mut c: CodePointIterator) -> bool {
    if c.at_end() {
        return false;
    }
    if c.get() == '.' as u32 {
        advance_without_violation(&mut c);
        return is_single_dot_path_segment(c);
    }
    if c.get() != '%' as u32 {
        return false;
    }
    advance_without_violation(&mut c);
    if c.at_end() || c.get() != '2' as u32 {
        return false;
    }
    advance_without_violation(&mut c);
    if c.at_end() {
        return false;
    }
    if is_ascii_alpha_caseless_equal(c.get(), 'e') {
        advance_without_violation(&mut c);
        return is_single_dot_path_segment(c);
    }
    false
}

/// Bit `PathStop` do `scanClassTable` (URLParser.cpp 346 e 361): precisa de codificação, é `/ \ ? #`
/// ou é não ASCII. Todo code unit acima de 0xFF tem a classe de U+00FF, que é parada.
fn is_path_stop(character: u32) -> bool {
    character > 0x7E
        || CHARACTER_CLASS_TABLE[character as usize] & PATH_ENCODE != 0
        || character == '/' as u32
        || character == '\\' as u32
        || character == '?' as u32
        || character == '#' as u32
}

/// `findPathRunEnd` (URLParser.cpp 598), ramo escalar (URLParser.cpp 635 a 645). Devolve o fim da corrida
/// e `lastSlash`. A barra só encerra a corrida se vier seguida de `.` ou `%` ou for o último caractere.
fn find_path_run_end(input: &[u16], begin: usize) -> (usize, Option<usize>) {
    let end = input.len();
    let mut last_slash = None;
    let mut cursor = begin;
    while cursor != end {
        let character = input[cursor] as u32;
        if !is_path_stop(character) {
            cursor += 1;
            continue;
        }
        if character == '/' as u32 && end - cursor > 1 && input[cursor + 1] != '.' as u16 && input[cursor + 1] != '%' as u16 {
            last_slash = Some(cursor);
            cursor += 1;
            continue;
        }
        return (cursor, last_slash);
    }
    (end, last_slash)
}

/// `findOpaquePathStopCharacterOrSlash` (URLParser.cpp 666) via `findStopCharacter<OpaquePathStop, '/'>`
/// (URLParser.cpp 560), ramo escalar. `OpaquePathStop` (URLParser.cpp 349 e 369): acima de 0x7E, C0, espaço, `?` e `#`.
fn find_opaque_path_stop_character_or_slash(input: &[u16], begin: usize) -> usize {
    let mut cursor = begin;
    while cursor != input.len() {
        let character = input[cursor] as u32;
        if character > 0x7E || character <= 0x20 || character == '?' as u32 || character == '#' as u32 || character == '/' as u32 {
            return cursor;
        }
        cursor += 1;
    }
    input.len()
}

/// `class URL` (URL.h), com os offsets do C++. `m_isValid`, `m_protocolIsInHTTPFamily` e
/// `m_hasOpaquePath` eram bit-fields; `m_portLength` também (3 bits, máximo 7).
#[derive(Clone, Debug, Default)]
pub struct URL {
    pub(crate) string: WtfString,
    pub(crate) is_valid: bool,
    pub(crate) protocol_is_in_http_family: bool,
    pub(crate) has_opaque_path: bool,
    pub(crate) port_length: u32,
    pub(crate) scheme_end: u32,
    pub(crate) user_start: u32,
    pub(crate) user_end: u32,
    pub(crate) password_end: u32,
    pub(crate) host_end: u32,
    pub(crate) path_after_last_slash: u32,
    pub(crate) path_end: u32,
    pub(crate) query_end: u32,
}

impl URL {
    /// `URL::invalidate()` (URL.h 362).
    pub fn invalidate(&mut self) {
        self.is_valid = false;
        self.protocol_is_in_http_family = false;
        self.has_opaque_path = false;
        self.port_length = 0;
        self.scheme_end = 0;
        self.user_start = 0;
        self.user_end = 0;
        self.password_end = 0;
        self.host_end = 0;
        self.path_after_last_slash = 0;
        self.path_end = 0;
        self.query_end = 0;
    }

    /// `URL::isNull()` (URL.h 374).
    pub fn is_null(&self) -> bool {
        self.string.is_null()
    }

    /// `URL::isValid()`.
    pub fn is_valid(&self) -> bool {
        self.is_valid
    }

    /// `URL::string()`.
    pub fn string(&self) -> &WtfString {
        &self.string
    }

    /// `URL::protocolIsInHTTPFamily()` (URL.h 189).
    pub fn protocol_is_in_http_family(&self) -> bool {
        self.protocol_is_in_http_family
    }

    /// `URL::hasCredentials()` (URL.h 395).
    pub fn has_credentials(&self) -> bool {
        self.password_end > self.user_start
    }

    /// `URL::hasQuery()` (URL.h 403).
    pub fn has_query(&self) -> bool {
        self.query_end > self.path_end
    }

    /// `URL::hasFragmentIdentifier()` (URL.h 408).
    pub fn has_fragment_identifier(&self) -> bool {
        self.is_valid && self.string.length() > self.query_end
    }

    /// `URL::hostStart()` (URL.cpp 519).
    pub fn host_start(&self) -> u32 {
        if self.password_end == self.user_start { self.password_end } else { self.password_end + 1 }
    }

    /// `URL::credentialsEnd()` (URL.cpp 524).
    pub fn credentials_end(&self) -> u32 {
        let mut end = self.password_end;
        if end != self.host_end && self.string.code_unit_at(end) == '@' as u16 {
            end += 1;
        }
        end
    }

    /// `URL::pathStart()`: depois da porta, que vem depois do host.
    pub fn path_start(&self) -> u32 {
        self.host_end + self.port_length
    }

    /// `URL::protocol()` (URL.cpp 143).
    pub fn protocol(&self) -> WtfString {
        if !self.is_valid {
            return WtfString::default();
        }
        self.string.substring(0, self.scheme_end)
    }

    /// `URL::host()` (URL.cpp 151).
    pub fn host(&self) -> WtfString {
        if !self.is_valid {
            return WtfString::default();
        }
        let start = self.host_start();
        self.string.substring(start, self.host_end - start)
    }

    /// `URL::port()` (URL.cpp 160): `parseInteger<uint16_t>` sobre os dígitos depois do `:`.
    pub fn port(&self) -> Option<u16> {
        if self.port_length == 0 {
            return None;
        }
        let digits = self.string.substring(self.host_end + 1, self.port_length - 1);
        let mut value: u32 = 0;
        if digits.length() == 0 {
            return None;
        }
        for i in 0..digits.length() {
            let c = digits.code_unit_at(i) as u32;
            if !(c >= '0' as u32 && c <= '9' as u32) {
                return None;
            }
            value = value * 10 + (c - '0' as u32);
            if value > u16::MAX as u32 {
                return None;
            }
        }
        Some(value as u16)
    }

    /// `URL::encodedUser()` (URL.cpp 279).
    pub fn encoded_user(&self) -> WtfString {
        self.string.substring(self.user_start, self.user_end - self.user_start)
    }

    /// `URL::encodedPassword()` (URL.cpp 284).
    pub fn encoded_password(&self) -> WtfString {
        if self.password_end == self.user_end {
            return WtfString::default();
        }
        self.string.substring(self.user_end + 1, self.password_end - self.user_end - 1)
    }

    /// `URL::path()` (URL.cpp 459).
    pub fn path(&self) -> WtfString {
        if !self.is_valid {
            return WtfString::default();
        }
        let path_start = self.path_start();
        self.string.substring(path_start, self.path_end - path_start)
    }

    /// `URL::query()` (URL.cpp 451).
    pub fn query(&self) -> WtfString {
        if self.query_end == self.path_end {
            return WtfString::default();
        }
        self.string.substring(self.path_end + 1, self.query_end - (self.path_end + 1))
    }

    /// `URL::fragmentIdentifier()` (URL.cpp 292).
    pub fn fragment_identifier(&self) -> WtfString {
        if !self.has_fragment_identifier() {
            return WtfString::default();
        }
        self.string.substring(self.query_end + 1, self.string.length() - (self.query_end + 1))
    }

    /// `URL::protocolIs(StringView)` (URL.cpp 433); comparação sem caixa em ASCII.
    pub fn protocol_is<T: Copy + Into<u32>>(&self, protocol: &[T]) -> bool {
        if !self.is_valid || self.scheme_end as usize != protocol.len() {
            return false;
        }
        for (i, p) in protocol.iter().enumerate() {
            if to_ascii_lower(self.string.code_unit_at(i as u32) as u32) != to_ascii_lower((*p).into()) {
                return false;
            }
        }
        true
    }

    /// `URL::protocolIsFile()`.
    pub fn protocol_is_file(&self) -> bool {
        self.protocol_is(&b"file"[..])
    }
}

/// `CodePointIterator<char16_t>` (text/CodePointIterator.h 36), sobre a entrada já cortada em
/// `endIndex`. `pos` é o `position()` em unidades de código.
#[derive(Clone, Copy)]
pub struct CodePointIterator<'a> {
    data: &'a [u16],
    pos: usize,
}

impl<'a> CodePointIterator<'a> {
    /// `CodePointIterator(std::span<const CharacterType>)` apontando para `pos`.
    pub fn new(data: &'a [u16], pos: usize) -> Self {
        CodePointIterator { data, pos }
    }

    /// `atEnd()`.
    pub fn at_end(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// `operator*` para `char16_t` (par substituto vira um ponto de código; órfão fica como está).
    pub fn get(&self) -> u32 {
        let lead = self.data[self.pos] as u32;
        if (0xD800..0xDC00).contains(&lead) && self.pos + 1 < self.data.len() {
            let trail = self.data[self.pos + 1] as u32;
            if (0xDC00..0xE000).contains(&trail) {
                return 0x10000 + ((lead - 0xD800) << 10) + (trail - 0xDC00);
            }
        }
        lead
    }

    /// `operator++`.
    pub fn advance_unit(&mut self) {
        self.pos += if self.get() >= 0x10000 { 2 } else { 1 };
    }

    /// `position()`.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// `codeUnitsSince(const CharacterType*)`, com a referência dada como índice.
    pub fn code_units_since(&self, reference: usize) -> usize {
        self.pos - reference
    }
}

/// `enum class State` (URLParser.cpp 1618).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    SchemeStart,
    Scheme,
    NoScheme,
    SpecialRelativeOrAuthority,
    PathOrAuthority,
    Relative,
    RelativeSlash,
    SpecialAuthoritySlashes,
    SpecialAuthorityIgnoreSlashes,
    AuthorityOrHost,
    Host,
    File,
    FileSlash,
    FileHost,
    FilePathStart,
    PathStart,
    Path,
    OpaquePath,
    Utf8Query,
    NonUtf8Query,
    Fragment,
}

/// `enum class URLParser::URLPart` (URLParser.cpp 1184). A ordem importa: o `copyURLPartsUntil`
/// do C++ usa `switch` com `[[fallthrough]]` do maior para o menor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum URLPart {
    SchemeEnd,
    UserStart,
    UserEnd,
    PasswordEnd,
    HostEnd,
    PortEnd,
    PathAfterLastSlash,
    PathEnd,
    QueryEnd,
}

/// `enum class URLParser::HostParsingResult` (URLParser.h 91).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostParsingResult {
    InvalidHost,
    IPv6WithPort,
    IPv6WithoutPort,
    IPv4WithPort,
    IPv4WithoutPort,
    DNSNameWithPort,
    DNSNameWithoutPort,
    NonSpecialHostWithoutPort,
    NonSpecialHostWithPort,
}

use crate::wtf::url_host::{
    parse_host_to_ascii_domain, parse_ipv4_host, parse_ipv6_host, serialize_ipv4, serialize_ipv6, IPv4Address,
    IPv4ParsingError, IPv6Address,
};

/// `URL::maxPortLength` (URL.h 282): `(1 << 3) - 1`.
pub const MAX_PORT_LENGTH: u32 = (1 << 3) - 1;

fn is_ascii_digit(character: u32) -> bool {
    character >= '0' as u32 && character <= '9' as u32
}

fn is_ascii_hex_digit(character: u32) -> bool {
    is_ascii_digit(character) || ((character | 0x20) >= 'a' as u32 && (character | 0x20) <= 'f' as u32)
}

/// Classe `HostNotDomainCharacter | HostStop` da `scanClassTable` (URLParser.cpp 365 a 377): o que não
/// é caractere de domínio nem depois de minúsculo (`HostNotPlain` sem a maiúscula), mais `/ \ ? # @`.
fn is_host_not_domain_character_or_stop(character: u32) -> bool {
    character > 0x7E
        || character <= 0x20
        || is_forbidden_domain_code_point(character)
        || character == '[' as u32
        || character == ']' as u32
        || character == ':' as u32
        || character == '/' as u32
        || character == '\\' as u32
        || character == '?' as u32
        || character == '#' as u32
        || character == '@' as u32
}

/// `scanClass(c) & IPv4NumberCharacter` (URLParser.cpp 379): dígito hexadecimal, `x` ou `X`.
fn is_ipv4_number_character(character: u16) -> bool {
    is_ascii_hex_digit(character as u32) || character == 'x' as u16 || character == 'X' as u16
}

/// `lastLabelMayBeANumber` (URLParser.cpp 415).
pub fn last_label_may_be_a_number(host: &[u16]) -> bool {
    let Some(&last) = host.last() else {
        return false;
    };
    if !is_ipv4_number_character(last) && last != '.' as u16 {
        return false;
    }
    let end = if last == '.' as u16 { host.len() - 1 } else { host.len() };
    let mut label = end;
    while label != 0 && is_ipv4_number_character(host[label - 1]) {
        label -= 1;
    }
    if label != 0 && host[label - 1] != '.' as u16 {
        return false;
    }
    label != end && is_ascii_digit(host[label] as u32)
}

/// `dnsNameEndsInNumber` (URLParser.cpp 3528), https://url.spec.whatwg.org/#ends-in-a-number-checker.
pub fn dns_name_ends_in_number(name: &[u16]) -> bool {
    // Lambda `containsOctalDecimalOrHexNumber` (3531).
    let contains_octal_decimal_or_hex_number = |segment: &[u16]| -> bool {
        let Some(&first) = segment.first() else {
            return false;
        };
        if !is_ascii_digit(first as u32) {
            return false;
        }
        if segment.len() == 1 {
            return true;
        }
        let second = segment[1];
        if (second == 'x' as u16 || second == 'X' as u16) && first == '0' as u16 {
            return segment[2..].iter().all(|&unit| is_ascii_hex_digit(unit as u32));
        }
        segment.iter().all(|&unit| is_ascii_digit(unit as u32))
    };

    let Some(mut last_dot_location) = name.iter().rposition(|&unit| unit == '.' as u16) else {
        return contains_octal_decimal_or_hex_number(name);
    };
    let mut last_segment_end = name.len();
    let mut has_previous_dot = true;
    if last_dot_location == last_segment_end - 1 {
        last_segment_end = last_dot_location;
        // `reverseFind('.', lastDotLocation - 1)`: com `lastDotLocation == 0` o C++ recua para `npos`
        // e o `substring` seguinte sai vazio.
        if last_dot_location == 0 {
            return false;
        }
        match name[..last_dot_location].iter().rposition(|&unit| unit == '.' as u16) {
            Some(previous) => last_dot_location = previous,
            None => has_previous_dot = false,
        }
    }
    let start = if has_previous_dot { last_dot_location + 1 } else { 0 };
    contains_octal_decimal_or_hex_number(&name[start..last_segment_end])
}

/// `class URLParser` (URLParser.h): só o estado que esta fatia usa.
pub struct URLParser {
    url: URL,
    input_string: WtfString,
    ascii_buffer: Vec<u8>,
    did_see_syntax_violation: bool,
    url_is_special: bool,
    url_is_file: bool,
    host_has_percent_or_non_ascii: bool,
}

impl URLParser {
    /// `URLParser::URLParser(URL&, String&&, const URL& base, const URLTextEncoding*)`
    /// (URLParser.cpp 1535), sem o ramo `needsNonSpecialDotSlash` (ver pendências).
    pub fn parse_url(input: &WtfString, base: &URL, non_utf8_query_encoding: QueryEncoding) -> URL {
        let mut parser = URLParser {
            url: URL::default(),
            input_string: input.clone(),
            ascii_buffer: Vec::new(),
            did_see_syntax_violation: false,
            url_is_special: false,
            url_is_file: false,
            host_has_percent_or_non_ascii: false,
        };
        if input.is_null() {
            if base.is_valid() && !base.has_opaque_path {
                parser.url = base.clone();
                parser.remove_fragment_identifier();
            }
            return parser.url;
        }
        let units: Vec<u16> = (0..input.length()).map(|i| input.code_unit_at(i)).collect();
        parser.parse(&units, base, non_utf8_query_encoding);
        // URLParser.cpp 1578.
        if parser.needs_non_special_dot_slash() {
            parser.add_non_special_dot_slash();
        }
        parser.url
    }

    /// `URL::removeFragmentIdentifier()`: corta a string em `m_queryEnd`.
    fn remove_fragment_identifier(&mut self) {
        if self.url.is_valid() {
            self.url.string = self.url.string.substring(0, self.url.query_end);
        }
    }

    /// `URLParser::failure()` (URLParser.cpp 1455).
    fn failure(&mut self) {
        self.url.invalidate();
        self.url.string = self.input_string.clone();
    }

    /// `URLParser::beginSyntaxViolation` (URLParser.cpp 1439): copia o já consumido.
    fn begin_syntax_violation(&mut self, input: &[u16], iterator: &CodePointIterator) {
        debug_assert!(!self.did_see_syntax_violation);
        self.did_see_syntax_violation = true;
        debug_assert!(self.ascii_buffer.is_empty());
        let code_units_to_copy = iterator.code_units_since(0);
        self.ascii_buffer.reserve(input.len() + 8);
        self.ascii_buffer.extend(input[..code_units_to_copy].iter().map(|&unit| unit as u8));
    }

    /// `URLParser::appendToASCIIBufferLowercased` (URLParser.cpp 820).
    fn append_to_ascii_buffer_lowercased(&mut self, characters: &[u16]) {
        if self.did_see_syntax_violation {
            self.ascii_buffer.extend(characters.iter().map(|&unit| to_ascii_lower(unit as u32) as u8));
        }
    }

    /// `URLParser::currentPosition(const CodePointIterator&)` (URLParser.cpp 1519).
    fn current_position(&self, iterator: &CodePointIterator) -> usize {
        if self.did_see_syntax_violation {
            return self.ascii_buffer.len();
        }
        iterator.code_units_since(0)
    }

    /// `URLParser::parsedDataView(size_t position)` (URLParser.cpp 1511).
    fn parsed_data_view_at(&self, position: usize) -> u16 {
        if self.did_see_syntax_violation {
            return self.ascii_buffer[position] as u16;
        }
        self.input_string.code_unit_at(position as u32)
    }

    /// `URLParser::urlLengthUntilPart` (URLParser.cpp 1196).
    fn url_length_until_part(url: &URL, part: URLPart) -> usize {
        (match part {
            URLPart::QueryEnd => url.query_end,
            URLPart::PathEnd => url.path_end,
            URLPart::PathAfterLastSlash => url.path_after_last_slash,
            URLPart::PortEnd => url.host_end + url.port_length,
            URLPart::HostEnd => url.host_end,
            URLPart::PasswordEnd => url.password_end,
            URLPart::UserEnd => url.user_end,
            URLPart::UserStart => url.user_start,
            URLPart::SchemeEnd => url.scheme_end,
        }) as usize
    }

    /// `URLParser::copyASCIIStringUntil` (URLParser.cpp 1219).
    fn copy_ascii_string_until(&mut self, string: &WtfString, length: usize) {
        assert!(length <= string.length() as usize);
        if string.is_null() {
            return;
        }
        debug_assert!(self.ascii_buffer.is_empty());
        let characters: Vec<u16> = (0..length as u32).map(|i| string.code_unit_at(i)).collect();
        debug_assert!(characters.iter().all(|&unit| unit < 0x80));
        self.append_span_to_ascii_buffer(&characters);
    }

    /// `URLParser::copyURLPartsUntil` (URLParser.cpp 1238).
    fn copy_url_parts_until(
        &mut self,
        input: &[u16],
        base: &URL,
        part: URLPart,
        iterator: &CodePointIterator,
        non_utf8_query_encoding: &mut Option<&dyn UrlTextEncoding>,
    ) {
        self.syntax_violation(input, iterator);

        self.ascii_buffer.clear();
        self.copy_ascii_string_until(&base.string, Self::url_length_until_part(base, part));
        // O `switch` com `[[fallthrough]]` do C++ (1244 a 1269): cada `if` equivale a um `case`.
        if part >= URLPart::QueryEnd {
            self.url.query_end = base.query_end;
        }
        if part >= URLPart::PathEnd {
            self.url.path_end = base.path_end;
        }
        if part >= URLPart::PathAfterLastSlash {
            self.url.path_after_last_slash = base.path_after_last_slash;
        }
        if part >= URLPart::PortEnd {
            self.url.port_length = base.port_length;
        }
        if part >= URLPart::HostEnd {
            self.url.host_end = base.host_end;
        }
        if part >= URLPart::PasswordEnd {
            self.url.password_end = base.password_end;
        }
        if part >= URLPart::UserEnd {
            self.url.user_end = base.user_end;
        }
        if part >= URLPart::UserStart {
            self.url.user_start = base.user_start;
        }
        self.url.is_valid = base.is_valid;
        self.url.protocol_is_in_http_family = base.protocol_is_in_http_family;
        self.url.scheme_end = base.scheme_end;

        match scheme_type(&self.ascii_buffer[..self.url.scheme_end as usize]) {
            Scheme::Ws | Scheme::Wss => {
                *non_utf8_query_encoding = None;
                self.url_is_special = true;
            }
            Scheme::File => {
                self.url_is_file = true;
                self.url_is_special = true;
            }
            Scheme::Ftp | Scheme::Http | Scheme::Https => {
                self.url_is_special = true;
            }
            Scheme::NonSpecial => {
                self.url_is_special = false;
                *non_utf8_query_encoding = None;
                let path_start = (self.url.host_end + self.url.port_length) as usize;
                if path_start + 2 < self.ascii_buffer.len()
                    && self.ascii_buffer[path_start] == b'/'
                    && self.ascii_buffer[path_start + 1] == b'.'
                    && self.ascii_buffer[path_start + 2] == b'/'
                {
                    self.ascii_buffer.drain(path_start + 1..path_start + 3);
                    self.url.path_after_last_slash = self.url.path_after_last_slash.max(2) - 2;
                    self.url.path_end = self.url.path_end.max(2) - 2;
                    self.url.query_end = self.url.query_end.max(2) - 2;
                }
            }
        }
    }

    /// Lambda `copyPlainCharacters` de `URLParser::parseAuthority` (URLParser.cpp 2917): pula a corrida
    /// de caracteres copiados sem mudança, deixando o iterador onde `advance` a partir do último deles deixaria.
    fn copy_plain_characters(&mut self, input: &[u16], iterator: &mut CodePointIterator) {
        let run_start = iterator.position();
        let end = iterator.data.len();
        let mut p = run_start;
        while p != end && !is_in_user_info_encode_set(iterator.data[p] as u32) {
            p += 1;
        }
        if p == run_start {
            return;
        }
        let run: Vec<u16> = iterator.data[run_start..p].to_vec();
        self.append_span_to_ascii_buffer(&run);
        *iterator = CodePointIterator::new(iterator.data, p - 1);
        self.advance(input, iterator);
    }

    /// `URLParser::parseAuthority` (URLParser.cpp 2909): user e password com percent-encode do userinfo.
    fn parse_authority(&mut self, input: &[u16], mut iterator: CodePointIterator) {
        if iterator.at_end() {
            self.syntax_violation(input, &iterator);
            self.url.user_end = self.current_position(&iterator) as u32;
            self.url.password_end = self.url.user_end;
            return;
        }
        while !iterator.at_end() {
            self.copy_plain_characters(input, &mut iterator);
            if iterator.at_end() {
                break;
            }
            if iterator.get() == ':' as u32 {
                self.url.user_end = self.current_position(&iterator) as u32;
                let iterator_at_colon = iterator;
                iterator.advance_unit();
                let mut tab_or_newline_after_colon = false;
                while !iterator.at_end() && is_tab_or_newline(iterator.get()) {
                    tab_or_newline_after_colon = true;
                    iterator.advance_unit();
                }
                if iterator.at_end() {
                    self.syntax_violation(input, &iterator_at_colon);
                    self.url.password_end = self.url.user_end;
                    if self.url.user_end > self.url.user_start {
                        self.append_to_ascii_buffer('@' as u32);
                    }
                    return;
                }
                if tab_or_newline_after_colon {
                    self.syntax_violation(input, &iterator_at_colon);
                }
                self.append_to_ascii_buffer(':' as u32);
                break;
            }
            self.utf8_percent_encode(input, &iterator, is_in_user_info_encode_set);
            self.advance(input, &mut iterator);
        }
        while !iterator.at_end() {
            self.copy_plain_characters(input, &mut iterator);
            if iterator.at_end() {
                break;
            }
            self.utf8_percent_encode(input, &iterator, is_in_user_info_encode_set);
            self.advance(input, &mut iterator);
        }
        self.url.password_end = self.current_position(&iterator) as u32;
        if self.url.user_end == 0 {
            self.url.user_end = self.url.password_end;
        }
        self.append_to_ascii_buffer('@' as u32);
    }

    /// `URLParser::parsedDataView(size_t position, size_t length)` (URLParser.cpp 1500): o trecho já
    /// produzido, do buffer ASCII se houve violação de sintaxe, da entrada senão.
    fn parsed_data_view(&self, start: usize, length: usize) -> Vec<u16> {
        (start..start + length).map(|position| self.parsed_data_view_at(position)).collect()
    }

    /// `URLParser::needsNonSpecialDotSlash` (URLParser.cpp 3389).
    fn needs_non_special_dot_slash(&self) -> bool {
        let path_start = self.url.host_end + self.url.port_length;
        !self.url_is_special
            && path_start == self.url.scheme_end + 1
            && path_start + 1 < self.url.string.length()
            && self.url.string.code_unit_at(path_start) == '/' as u16
            && self.url.string.code_unit_at(path_start + 1) == '/' as u16
    }

    /// `URLParser::addNonSpecialDotSlash` (URLParser.cpp 3399): insere `./` depois da primeira barra.
    fn add_non_special_dot_slash(&mut self) {
        let old_path_start = self.url.host_end + self.url.port_length;
        let old_string = &self.url.string;
        let mut units: Vec<u16> = (0..=old_path_start).map(|i| old_string.code_unit_at(i)).collect();
        units.extend(['.' as u16, '/' as u16]);
        units.extend((old_path_start + 1..old_string.length()).map(|i| old_string.code_unit_at(i)));
        self.url.string = WtfString::from_utf16(&units);
        self.url.path_after_last_slash += 2;
        self.url.path_end += 2;
        self.url.query_end += 2;
    }

    /// `URLParser::serializeIPv4` (URLParser.cpp 2982): cada anexo passa por `appendToASCIIBuffer`,
    /// que só escreve se houve violação de sintaxe.
    fn serialize_ipv4_to_ascii_buffer(&mut self, address: IPv4Address) {
        if self.did_see_syntax_violation {
            serialize_ipv4(address, &mut self.ascii_buffer);
        }
    }

    /// `URLParser::serializeIPv6` (URLParser.cpp 3038), mesma regra do `serialize_ipv4_to_ascii_buffer`.
    fn serialize_ipv6_to_ascii_buffer(&mut self, address: &IPv6Address) {
        if self.did_see_syntax_violation {
            serialize_ipv6(address, &mut self.ascii_buffer);
        }
    }

    /// `URLParser::parsePort` (URLParser.cpp 3469). Consome o resto da entrada: o iterador sai no fim.
    fn parse_port(&mut self, input: &[u16], iterator: &mut CodePointIterator) -> bool {
        if self.url_is_file {
            return false;
        }
        debug_assert!(iterator.get() == ':' as u32);
        let colon_iterator = *iterator;
        let data = iterator.data;
        let end = data.len();
        let mut p = iterator.position() + 1;
        *iterator = CodePointIterator::new(data, end);
        while p != end && is_tab_or_newline(data[p] as u32) {
            self.syntax_violation(input, &colon_iterator);
            p += 1;
        }
        if p == end {
            let port_length = self.current_position(&colon_iterator) as u32 - self.url.host_end;
            assert!(port_length <= MAX_PORT_LENGTH);
            self.url.port_length = port_length;
            self.syntax_violation(input, &colon_iterator);
            return true;
        }
        let mut port: u32 = 0;
        let mut digit_count = 0usize;
        let mut leading_zeros = false;
        while p != end {
            let unit = data[p] as u32;
            p += 1;
            if is_tab_or_newline(unit) {
                self.syntax_violation(input, &colon_iterator);
                continue;
            }
            if !is_ascii_digit(unit) {
                return false;
            }
            if unit == '0' as u32 && digit_count == 0 {
                leading_zeros = true;
            }
            digit_count += 1;
            port = port * 10 + unit - '0' as u32;
            if port > u16::MAX as u32 {
                return false;
            }
        }
        if port != 0 && leading_zeros {
            self.syntax_violation(input, &colon_iterator);
        }
        if port == 0 && digit_count > 1 {
            self.syntax_violation(input, &colon_iterator);
        }
        let scheme = self.parsed_data_view(0, self.url.scheme_end as usize);
        if default_port(scheme_type(&scheme[..])) == Some(port) {
            self.syntax_violation(input, &colon_iterator);
        } else {
            self.append_to_ascii_buffer(':' as u32);
            let digits: Vec<u16> = port.to_string().bytes().map(|digit| digit as u16).collect();
            self.append_span_to_ascii_buffer(&digits);
        }
        let port_length = self.current_position(iterator) as u32 - self.url.host_end;
        assert!(port_length <= MAX_PORT_LENGTH);
        self.url.port_length = port_length;
        true
    }

    /// Cauda comum dos ramos IPv4 de `parseHostAndPort` (3657 a 3666 e 3861 a 3869): grava `m_hostEnd`
    /// e lê a porta, se houver.
    fn finish_ipv4_host(&mut self, input: &[u16], mut iterator: CodePointIterator) -> HostParsingResult {
        self.url.host_end = self.current_position(&iterator) as u32;
        if iterator.at_end() {
            self.url.port_length = 0;
            return HostParsingResult::IPv4WithoutPort;
        }
        if self.parse_port(input, &mut iterator) { HostParsingResult::IPv4WithPort } else { HostParsingResult::InvalidHost }
    }

    /// Cauda comum dos ramos DNS de `parseHostAndPort` (3690 a 3699 e 3875 a 3885).
    fn finish_dns_host(&mut self, input: &[u16], mut iterator: CodePointIterator, may_end_in_number: bool) -> HostParsingResult {
        self.url.host_end = self.current_position(&iterator) as u32;
        if may_end_in_number {
            let host_start = self.url.host_start() as usize;
            let name = self.parsed_data_view(host_start, self.url.host_end as usize - host_start);
            if dns_name_ends_in_number(&name) {
                return HostParsingResult::InvalidHost;
            }
        }
        if !iterator.at_end() {
            return if self.parse_port(input, &mut iterator) {
                HostParsingResult::DNSNameWithPort
            } else {
                HostParsingResult::InvalidHost
            };
        }
        self.url.port_length = 0;
        HostParsingResult::DNSNameWithoutPort
    }

    /// `URLParser::parseHostAndPort` (URLParser.cpp 3559). O C++ separa o ramo rápido (3627, sem `%` nem
    /// não-ASCII) do lento (3702); a varredura SIMD de 3709 a 3766 vira o `parse_host_to_ascii_domain`.
    fn parse_host_and_port(&mut self, input: &[u16], mut iterator: CodePointIterator) -> HostParsingResult {
        if iterator.at_end() || iterator.get() == ':' as u32 {
            return HostParsingResult::InvalidHost;
        }
        let data = iterator.data;
        let end = data.len();
        if iterator.get() == '[' as u32 {
            // 3565: literal IPv6.
            let address_begin = iterator.position() + 1;
            let mut address_end = address_begin;
            let mut has_tab_or_newline = false;
            while address_end != end && data[address_end] != ']' as u16 {
                has_tab_or_newline |= is_tab_or_newline(data[address_end] as u32);
                address_end += 1;
            }
            if address_end == end {
                return HostParsingResult::InvalidHost;
            }
            let mut ipv6_end = CodePointIterator::new(data, address_end);
            let mut address_characters: Vec<u16> = data[address_begin..address_end].to_vec();
            if has_tab_or_newline {
                self.syntax_violation(input, &iterator);
                let mut filtered: Vec<u16> = Vec::new();
                for &character in &address_characters {
                    if is_tab_or_newline(character as u32) {
                        continue;
                    }
                    // O endereço válido mais longo, 0000:0000:0000:0000:0000:0000:000.000.000.000, tem 45.
                    if filtered.len() == 45 {
                        return HostParsingResult::InvalidHost;
                    }
                    filtered.push(character);
                }
                address_characters = filtered;
            }
            let iterator_for_violation = iterator;
            let address = parse_ipv6_host(&address_characters, &mut || self.syntax_violation(input, &iterator_for_violation));
            let Some(address) = address else {
                return HostParsingResult::InvalidHost;
            };
            self.serialize_ipv6_to_ascii_buffer(&address);
            if !ipv6_end.at_end() {
                self.advance(input, &mut ipv6_end);
                self.url.host_end = self.current_position(&ipv6_end) as u32;
                if !ipv6_end.at_end() && ipv6_end.get() == ':' as u32 {
                    return if self.parse_port(input, &mut ipv6_end) {
                        HostParsingResult::IPv6WithPort
                    } else {
                        HostParsingResult::InvalidHost
                    };
                }
                self.url.port_length = 0;
                return if ipv6_end.at_end() { HostParsingResult::IPv6WithoutPort } else { HostParsingResult::InvalidHost };
            }
            self.url.host_end = self.current_position(&ipv6_end) as u32;
            return HostParsingResult::IPv6WithoutPort;
        }

        if !self.url_is_special {
            // 3607: host opaco.
            while !iterator.at_end() {
                if is_tab_or_newline(iterator.get()) {
                    self.syntax_violation(input, &iterator);
                    iterator.advance_unit();
                    continue;
                }
                if iterator.get() == ':' as u32 {
                    break;
                }
                if is_forbidden_host_code_point(iterator.get() as u16) && iterator.get() != '%' as u32 {
                    return HostParsingResult::InvalidHost;
                }
                self.utf8_percent_encode(input, &iterator, is_in_c0_control_encode_set);
                iterator.advance_unit();
            }
            self.url.host_end = self.current_position(&iterator) as u32;
            if iterator.at_end() {
                self.url.port_length = 0;
                return HostParsingResult::NonSpecialHostWithoutPort;
            }
            return if self.parse_port(input, &mut iterator) {
                HostParsingResult::NonSpecialHostWithPort
            } else {
                HostParsingResult::InvalidHost
            };
        }

        let host_iterator = iterator;
        let host_begin = iterator.position();
        if !self.host_has_percent_or_non_ascii {
            // 3627: ramo rápido, só ASCII sem `%`.
            let mut host_end = host_begin;
            let mut has_tab_or_newline = false;
            let mut has_uppercase = false;
            while host_end != end {
                let character = data[host_end] as u32;
                if character == ':' as u32 {
                    break;
                }
                if is_host_not_domain_character_or_stop(character) {
                    if !is_tab_or_newline(character) {
                        return HostParsingResult::InvalidHost;
                    }
                    has_tab_or_newline = true;
                }
                has_uppercase |= is_ascii_upper(character);
                host_end += 1;
            }
            let host = &data[host_begin..host_end];
            iterator = CodePointIterator::new(data, host_end);

            let may_be_ipv4_or_end_in_a_number = has_tab_or_newline || last_label_may_be_a_number(host);
            if may_be_ipv4_or_end_in_a_number {
                match parse_ipv4_host(host, &mut || self.syntax_violation(input, &host_iterator)) {
                    Ok(address) => {
                        self.serialize_ipv4_to_ascii_buffer(address);
                        return self.finish_ipv4_host(input, iterator);
                    }
                    Err(IPv4ParsingError::Failure) => return HostParsingResult::InvalidHost,
                    Err(IPv4ParsingError::NotIPv4) => {}
                }
            }
            if !has_uppercase && !has_tab_or_newline {
                self.append_span_to_ascii_buffer(host);
            } else if !has_tab_or_newline {
                let first_uppercase = host_begin + host.iter().position(|&unit| is_ascii_upper(unit as u32)).unwrap();
                self.append_span_to_ascii_buffer(&data[host_begin..first_uppercase]);
                self.syntax_violation(input, &CodePointIterator::new(data, first_uppercase));
                self.append_to_ascii_buffer_lowercased(&data[first_uppercase..host_end]);
            } else {
                let mut cursor = host_iterator;
                while cursor.position() != iterator.position() {
                    if is_tab_or_newline(cursor.get()) {
                        self.syntax_violation(input, &cursor);
                        cursor.advance_unit();
                        continue;
                    }
                    if is_ascii_upper(cursor.get()) {
                        self.syntax_violation(input, &cursor);
                    }
                    self.append_to_ascii_buffer(to_ascii_lower(cursor.get()));
                    cursor.advance_unit();
                }
            }
            return self.finish_dns_host(input, iterator, may_be_ipv4_or_end_in_a_number);
        }

        // 3702: ramo lento. O host termina no primeiro `:` (literais IPv6 já foram tratados acima).
        let host_end = data[host_begin..].iter().position(|&unit| unit == ':' as u16).map_or(end, |offset| host_begin + offset);
        iterator = CodePointIterator::new(data, host_end);
        let host = &data[host_begin..host_end];
        // `syntaxViolation(hostBegin)` dentro do callback; o espelho `seen` serve ao `Fn` que lê `m_didSeeSyntaxViolation`.
        let seen = std::cell::Cell::new(self.did_see_syntax_violation);
        let ascii_domain = parse_host_to_ascii_domain(host, &|| seen.get(), &mut || {
            self.syntax_violation(input, &host_iterator);
            seen.set(true);
        });
        let Some(ascii_domain) = ascii_domain else {
            return HostParsingResult::InvalidHost;
        };
        let ascii_units: Vec<u16> = ascii_domain.iter().map(|&byte| byte as u16).collect();

        let may_be_ipv4_or_end_in_a_number = last_label_may_be_a_number(&ascii_units);
        if may_be_ipv4_or_end_in_a_number {
            match parse_ipv4_host(&ascii_units, &mut || self.syntax_violation(input, &host_iterator)) {
                Ok(address) => {
                    self.serialize_ipv4_to_ascii_buffer(address);
                    return self.finish_ipv4_host(input, iterator);
                }
                Err(IPv4ParsingError::Failure) => return HostParsingResult::InvalidHost,
                Err(IPv4ParsingError::NotIPv4) => {}
            }
        }
        self.append_span_to_ascii_buffer(&ascii_units);
        self.finish_dns_host(input, iterator, may_be_ipv4_or_end_in_a_number)
    }

    /// `URLParser::appendWindowsDriveLetter` (URLParser.cpp 834): anexa `X:` normalizado (`|` vira `:`) e
    /// descarta o que o buffer tinha de caminho além da primeira barra.
    fn append_windows_drive_letter(&mut self, input: &[u16], iterator: &mut CodePointIterator) {
        let length_with_only_one_slash_in_path = self.url.host_end + self.url.port_length + 1;
        if self.url.path_after_last_slash > length_with_only_one_slash_in_path {
            self.syntax_violation(input, iterator);
            self.url.path_after_last_slash = length_with_only_one_slash_in_path;
            self.ascii_buffer.resize(length_with_only_one_slash_in_path as usize, 0);
        }
        debug_assert!(is_windows_drive_letter(*iterator));
        self.append_to_ascii_buffer(iterator.get());
        self.advance(input, iterator);
        debug_assert!(!iterator.at_end());
        debug_assert!(iterator.get() == ':' as u32 || iterator.get() == '|' as u32);
        if iterator.get() == '|' as u32 {
            self.syntax_violation(input, iterator);
        }
        self.append_to_ascii_buffer(':' as u32);
        self.advance(input, iterator);
    }

    /// `URLParser::copyBaseWindowsDriveLetter` (URLParser.cpp 853): se o caminho da base `file:` começa por
    /// uma letra de unidade, copia-a para o buffer.
    fn copy_base_windows_drive_letter(&mut self, input: &[u16], base: &URL) -> bool {
        if base.protocol_is_file() {
            let start = (base.host_end + base.port_length) as usize;
            assert!(start < base.string.length() as usize);
            let characters: Vec<u16> = (start as u32 + 1..base.string.length()).map(|i| base.string.code_unit_at(i)).collect();
            let mut c = CodePointIterator::new(&characters, 0);
            if is_windows_drive_letter(c) {
                self.append_windows_drive_letter(input, &mut c);
                return true;
            }
        }
        false
    }

    /// `URLParser::shouldCopyFileURL` (URLParser.cpp 877).
    fn should_copy_file_url(&mut self, input: &[u16], mut iterator: CodePointIterator) -> bool {
        if !is_windows_drive_letter(iterator) {
            return true;
        }
        if iterator.at_end() {
            return false;
        }
        self.advance(input, &mut iterator);
        if iterator.at_end() {
            return true;
        }
        self.advance(input, &mut iterator);
        if iterator.at_end() {
            return true;
        }
        !is_slash_question_or_hash(iterator.get())
    }

    /// `URLParser::advance(CodePointIterator&)` (URLParser.cpp 733): a posição da violação é o próprio iterador.
    fn advance(&mut self, input: &[u16], iterator: &mut CodePointIterator) {
        let position_for_violation = *iterator;
        self.advance_for(input, iterator, &position_for_violation);
    }

    /// `URLParser::consumeSingleDotPathSegment` (URLParser.cpp 1358): consome `.` ou `%2e` e a barra que segue.
    fn consume_single_dot_path_segment(&mut self, input: &[u16], c: &mut CodePointIterator) {
        debug_assert!(is_single_dot_path_segment(*c));
        if c.get() == '.' as u32 {
            self.advance(input, c);
        } else {
            debug_assert!(c.get() == '%' as u32);
            self.advance(input, c);
            debug_assert!(c.get() == '2' as u32);
            self.advance(input, c);
            debug_assert!(is_ascii_alpha_caseless_equal(c.get(), 'e'));
            self.advance(input, c);
        }
        if !c.at_end() {
            if c.get() == '/' as u32 || c.get() == '\\' as u32 {
                self.advance(input, c);
            } else {
                debug_assert!(c.get() == '?' as u32 || c.get() == '#' as u32);
            }
        }
    }

    /// `URLParser::consumeDoubleDotPathSegment` (URLParser.cpp 1386): consome o primeiro ponto e depois um segmento de ponto simples.
    fn consume_double_dot_path_segment(&mut self, input: &[u16], c: &mut CodePointIterator) {
        debug_assert!(is_double_dot_path_segment(*c));
        if c.get() == '.' as u32 {
            self.advance(input, c);
        } else {
            debug_assert!(c.get() == '%' as u32);
            self.advance(input, c);
            debug_assert!(c.get() == '2' as u32);
            self.advance(input, c);
            debug_assert!(is_ascii_alpha_caseless_equal(c.get(), 'e'));
            self.advance(input, c);
        }
        self.consume_single_dot_path_segment(input, c);
    }

    /// `URLParser::shouldPopPath` (URLParser.cpp 1402): em `file:` não se remove a letra de unidade do Windows logo após o host.
    fn should_pop_path(&self, new_path_after_last_slash: usize) -> bool {
        debug_assert!(self.did_see_syntax_violation);
        if !self.url_is_file {
            return true;
        }
        debug_assert!(self.url.path_after_last_slash as usize <= self.ascii_buffer.len());
        let component_to_pop: Vec<u16> = self.ascii_buffer
            [new_path_after_last_slash..self.url.path_after_last_slash as usize]
            .iter()
            .map(|&byte| byte as u16)
            .collect();
        let iterator = CodePointIterator::new(&component_to_pop, 0);
        if new_path_after_last_slash == (self.url.host_end + self.url.port_length + 1) as usize
            && is_windows_drive_letter(iterator)
        {
            return false;
        }
        true
    }

    /// `URLParser::popPath` (URLParser.cpp 1415): descarta o último segmento do caminho já escrito em `m_asciiBuffer`.
    fn pop_path(&mut self) {
        debug_assert!(self.did_see_syntax_violation);
        if self.url.path_after_last_slash > self.url.host_end + self.url.port_length + 1 {
            let mut new_path_after_last_slash = (self.url.path_after_last_slash - 1) as usize;
            if self.ascii_buffer[new_path_after_last_slash] == b'/' {
                new_path_after_last_slash -= 1;
            }
            while new_path_after_last_slash > (self.url.host_end + self.url.port_length) as usize
                && self.ascii_buffer[new_path_after_last_slash] != b'/'
            {
                new_path_after_last_slash -= 1;
            }
            new_path_after_last_slash += 1;
            if self.should_pop_path(new_path_after_last_slash) {
                self.url.path_after_last_slash = new_path_after_last_slash as u32;
            }
        }
        self.ascii_buffer.resize(self.url.path_after_last_slash as usize, 0);
    }

    /// `URLParser::parse<CharacterType>` (URLParser.cpp 1583), do começo até os estados
    /// `SchemeStart`, `Scheme` e `NoScheme`.
    fn parse(&mut self, input: &[u16], base: &URL, non_utf8_query_encoding: QueryEncoding) {
        let mut query_buffer: Vec<u16> = Vec::new();
        let mut end_index = input.len();
        // `URLTextEncodingSentinelAllowingC0AtEnd` vira `nullptr` sem cortar os C0/espaços do fim (URLParser.cpp 1592).
        let sentinel_allowing_c0_at_end = matches!(non_utf8_query_encoding, QueryEncoding::SentinelAllowingC0AtEnd);
        let mut non_utf8_query_encoding: Option<&dyn UrlTextEncoding> = match non_utf8_query_encoding {
            QueryEncoding::Encoding(encoding) => Some(encoding),
            QueryEncoding::None | QueryEncoding::SentinelAllowingC0AtEnd => None,
        };
        if !sentinel_allowing_c0_at_end {
            while end_index > 0 && is_c0_control_or_space(input[end_index - 1] as u32) {
                let iterator = CodePointIterator::new(&input[..end_index], 0);
                self.syntax_violation(input, &iterator);
                end_index -= 1;
            }
        }
        let input = &input[..end_index];
        let mut c = CodePointIterator::new(input, 0);
        let mut authority_or_host_begin = CodePointIterator::new(input, 0);
        let mut query_begin = CodePointIterator::new(input, 0);
        while !c.at_end() && is_c0_control_or_space(c.get()) {
            self.syntax_violation(input, &c);
            c.advance_unit();
        }
        let begin_after_control_and_space = c.position();
        let iterator_at = |position: usize| CodePointIterator::new(input, position);

        let mut state = State::SchemeStart;

        // Pendente: o "straight-line pass" de URLParser.cpp 1650 a 1801 (atalho de desempenho).

        while !c.at_end() {
            if is_tab_or_newline(c.get()) {
                self.syntax_violation(input, &c);
                c.advance_unit();
                continue;
            }

            let mut run_scheme_body = state == State::Scheme;
            if state == State::SchemeStart {
                // URLParser.cpp 1808.
                let mut reached_scheme_end = false;
                if is_ascii_alpha(c.get()) {
                    let start = c.position();
                    let mut p = start;
                    let mut first_uppercase: Option<usize> = None;
                    while p != input.len() {
                        let unit = input[p] as u32;
                        // A maiúscula também é caractere válido de esquema: precisa ser vista antes, senão o
                        // esquema `HTTP:` passa sem baixar a caixa e não é reconhecido como especial.
                        if is_ascii_upper(unit) {
                            if first_uppercase.is_none() {
                                first_uppercase = Some(p);
                            }
                            p += 1;
                            continue;
                        }
                        if !is_valid_scheme_character(unit) {
                            break;
                        }
                        p += 1;
                    }
                    if p != input.len() && input[p] == ':' as u16 {
                        match first_uppercase {
                            None => self.append_span_to_ascii_buffer(&input[start..p]),
                            Some(first) => {
                                self.append_span_to_ascii_buffer(&input[start..first]);
                                self.syntax_violation(input, &iterator_at(first));
                                self.append_to_ascii_buffer_lowercased(&input[first..p]);
                            }
                        }
                        c = iterator_at(p);
                        state = State::Scheme;
                        reached_scheme_end = true;
                    }
                }
                if !reached_scheme_end {
                    if is_ascii_alpha(c.get()) {
                        if is_ascii_upper(c.get()) {
                            self.syntax_violation(input, &c);
                        }
                        self.append_to_ascii_buffer(to_ascii_lower(c.get()));
                        self.advance(input, &mut c);
                        if c.at_end() {
                            self.ascii_buffer.clear();
                            state = State::NoScheme;
                            c = iterator_at(begin_after_control_and_space);
                            continue;
                        }
                        state = State::Scheme;
                    } else {
                        state = State::NoScheme;
                    }
                    continue;
                }
                debug_assert!(c.get() == ':' as u32);
                run_scheme_body = true;
            }

            if run_scheme_body {
                // URLParser.cpp 1856.
                let ch = c.get();
                if is_valid_scheme_character(ch) {
                    if is_ascii_upper(ch) {
                        self.syntax_violation(input, &c);
                    }
                    self.append_to_ascii_buffer(to_ascii_lower(ch));
                } else if ch == ':' as u32 {
                    let scheme_end = self.current_position(&c);
                    if scheme_end > MAX_SCHEME_LENGTH as usize {
                        self.failure();
                        return;
                    }
                    self.url.scheme_end = scheme_end as u32;
                    self.append_to_ascii_buffer(':' as u32);
                    let url_scheme_type = if self.did_see_syntax_violation {
                        scheme_type(&self.ascii_buffer[..scheme_end])
                    } else {
                        scheme_type(&input[..scheme_end])
                    };
                    self.url.protocol_is_in_http_family = matches!(url_scheme_type, Scheme::Http | Scheme::Https);
                    match url_scheme_type {
                        Scheme::File => {
                            self.url_is_special = true;
                            self.url_is_file = true;
                            state = State::File;
                            c.advance_unit();
                        }
                        Scheme::Ws | Scheme::Wss | Scheme::Http | Scheme::Https | Scheme::Ftp => {
                            if matches!(url_scheme_type, Scheme::Ws | Scheme::Wss) {
                                non_utf8_query_encoding = None;
                            }
                            self.url_is_special = true;
                            let p = c.position();
                            let p3 = input.get(p + 3).map(|&unit| unit as u32);
                            if input.len() - p > 3
                                && input[p + 1] == '/' as u16
                                && input[p + 2] == '/' as u16
                                && !is_tab_or_newline(p3.unwrap_or(0))
                                && p3 != Some('/' as u32)
                                && p3 != Some('\\' as u32)
                            {
                                // Equivale a passar por SpecialAuthoritySlashes ou
                                // SpecialRelativeOrAuthority e depois SpecialAuthorityIgnoreSlashes.
                                self.append_span_to_ascii_buffer(&['/' as u16, '/' as u16]);
                                c = iterator_at(p + 3);
                                self.url.user_start = self.current_position(&c) as u32;
                                authority_or_host_begin = c;
                                state = State::AuthorityOrHost;
                            } else {
                                let parsed_scheme: Vec<u32> = if self.did_see_syntax_violation {
                                    self.ascii_buffer[..self.url.scheme_end as usize].iter().map(|&b| b as u32).collect()
                                } else {
                                    input[..self.url.scheme_end as usize].iter().map(|&u| u as u32).collect()
                                };
                                state = if base.is_valid() && base.protocol_is(&parsed_scheme) {
                                    State::SpecialRelativeOrAuthority
                                } else {
                                    State::SpecialAuthoritySlashes
                                };
                                c.advance_unit();
                            }
                        }
                        Scheme::NonSpecial => {
                            non_utf8_query_encoding = None;
                            let mut maybe_slash = c;
                            self.advance(input, &mut maybe_slash);
                            if !maybe_slash.at_end() && maybe_slash.get() == '/' as u32 {
                                self.append_to_ascii_buffer('/' as u32);
                                c = maybe_slash;
                                state = State::PathOrAuthority;
                                debug_assert!(c.get() == '/' as u32);
                                c.advance_unit();
                                self.url.user_start = self.current_position(&c) as u32;
                            } else {
                                c.advance_unit();
                                self.url.user_start = self.current_position(&c) as u32;
                                self.url.user_end = self.url.user_start;
                                self.url.password_end = self.url.user_start;
                                self.url.host_end = self.url.user_start;
                                self.url.port_length = 0;
                                self.url.path_after_last_slash = self.url.user_start;
                                self.url.has_opaque_path = true;
                                state = State::OpaquePath;
                            }
                        }
                    }
                    continue;
                } else {
                    self.ascii_buffer.clear();
                    state = State::NoScheme;
                    c = iterator_at(begin_after_control_and_space);
                    continue;
                }
                self.advance(input, &mut c);
                if c.at_end() {
                    self.ascii_buffer.clear();
                    state = State::NoScheme;
                    c = iterator_at(begin_after_control_and_space);
                }
                continue;
            }

            match state {
                State::SchemeStart | State::Scheme => unreachable!("tratados acima"),
                State::NoScheme => {
                    // URLParser.cpp 1942.
                    if !base.is_valid() || (base.has_opaque_path && c.get() != '#' as u32) {
                        self.failure();
                        return;
                    }
                    if base.has_opaque_path && c.get() == '#' as u32 {
                        self.copy_url_parts_until(input, base, URLPart::QueryEnd, &c, &mut non_utf8_query_encoding);
                        state = State::Fragment;
                        self.append_to_ascii_buffer('#' as u32);
                        c.advance_unit();
                        continue;
                    }
                    if !base.protocol_is_file() {
                        let base_protocol = base.protocol();
                        let units: Vec<u16> =
                            (0..base_protocol.length()).map(|i| base_protocol.code_unit_at(i)).collect();
                        self.url_is_special = is_special_scheme(&units);
                        state = State::Relative;
                        continue;
                    }
                    state = State::File;
                }
                State::SpecialRelativeOrAuthority => {
                    // URLParser.cpp 1962.
                    if c.get() == '/' as u32 {
                        self.append_to_ascii_buffer('/' as u32);
                        self.advance(input, &mut c);
                        if !c.at_end() && c.get() == '/' as u32 {
                            self.append_to_ascii_buffer('/' as u32);
                            state = State::SpecialAuthorityIgnoreSlashes;
                            c.advance_unit();
                        } else {
                            state = State::RelativeSlash;
                        }
                    } else {
                        state = State::Relative;
                    }
                }
                State::PathOrAuthority => {
                    // URLParser.cpp 1976.
                    if c.get() == '/' as u32 {
                        self.append_to_ascii_buffer('/' as u32);
                        state = State::AuthorityOrHost;
                        self.advance(input, &mut c);
                        self.url.user_start = self.current_position(&c) as u32;
                        authority_or_host_begin = c;
                    } else {
                        debug_assert!(self.parsed_data_view_at(self.current_position(&c) - 1) == '/' as u16);
                        self.url.user_start = (self.current_position(&c) - 1) as u32;
                        self.url.user_end = self.url.user_start;
                        self.url.password_end = self.url.user_start;
                        self.url.host_end = self.url.user_start;
                        self.url.port_length = 0;
                        self.url.path_after_last_slash = self.url.user_start + 1;
                        state = State::Path;
                    }
                }
                State::Relative => {
                    // URLParser.cpp 1999.
                    let ch = c.get();
                    if ch == '/' as u32 {
                        state = State::RelativeSlash;
                        c.advance_unit();
                    } else if ch == '?' as u32 {
                        self.copy_url_parts_until(input, base, URLPart::PathEnd, &c, &mut non_utf8_query_encoding);
                        self.append_to_ascii_buffer('?' as u32);
                        c.advance_unit();
                        if non_utf8_query_encoding.is_some() {
                            query_begin = c;
                            state = State::NonUtf8Query;
                        } else {
                            state = State::Utf8Query;
                        }
                    } else if ch == '#' as u32 {
                        self.copy_url_parts_until(input, base, URLPart::QueryEnd, &c, &mut non_utf8_query_encoding);
                        self.append_to_ascii_buffer('#' as u32);
                        state = State::Fragment;
                        c.advance_unit();
                    } else if ch == '\\' as u32 && self.url_is_special {
                        state = State::RelativeSlash;
                        c.advance_unit();
                    } else {
                        self.copy_url_parts_until(
                            input,
                            base,
                            URLPart::PathAfterLastSlash,
                            &c,
                            &mut non_utf8_query_encoding,
                        );
                        let position = self.current_position(&c);
                        if (position != 0 && self.parsed_data_view_at(position - 1) != '/' as u16)
                            || (base.host().is_empty() && base.path().is_empty())
                        {
                            self.append_to_ascii_buffer('/' as u32);
                            self.url.path_after_last_slash = self.current_position(&c) as u32;
                        }
                        state = State::Path;
                    }
                }
                State::RelativeSlash => {
                    // URLParser.cpp 2036.
                    if c.get() == '/' as u32 || (c.get() == '\\' as u32 && self.url_is_special) {
                        c.advance_unit();
                        self.copy_url_parts_until(input, base, URLPart::SchemeEnd, &c, &mut non_utf8_query_encoding);
                        self.append_span_to_ascii_buffer(&[':' as u16, '/' as u16, '/' as u16]);
                        if self.url_is_special {
                            state = State::SpecialAuthorityIgnoreSlashes;
                        } else {
                            self.url.user_start = self.current_position(&c) as u32;
                            state = State::AuthorityOrHost;
                            authority_or_host_begin = c;
                        }
                    } else {
                        self.copy_url_parts_until(input, base, URLPart::PortEnd, &c, &mut non_utf8_query_encoding);
                        self.append_to_ascii_buffer('/' as u32);
                        self.url.path_after_last_slash = base.host_end + base.port_length + 1;
                        state = State::Path;
                    }
                }
                State::SpecialAuthoritySlashes => {
                    // URLParser.cpp 2056.
                    if c.get() == '/' as u32 || c.get() == '\\' as u32 {
                        if c.get() == '\\' as u32 {
                            self.syntax_violation(input, &c);
                        }
                        self.append_to_ascii_buffer('/' as u32);
                        self.advance(input, &mut c);
                        if !c.at_end() && (c.get() == '/' as u32 || c.get() == '\\' as u32) {
                            if c.get() == '\\' as u32 {
                                self.syntax_violation(input, &c);
                            }
                            c.advance_unit();
                            self.append_to_ascii_buffer('/' as u32);
                        } else {
                            self.syntax_violation(input, &c);
                            self.append_to_ascii_buffer('/' as u32);
                        }
                    } else {
                        self.syntax_violation(input, &c);
                        self.append_span_to_ascii_buffer(&['/' as u16, '/' as u16]);
                    }
                    state = State::SpecialAuthorityIgnoreSlashes;
                }
                State::SpecialAuthorityIgnoreSlashes => {
                    // URLParser.cpp 2078.
                    if c.get() == '/' as u32 || c.get() == '\\' as u32 {
                        self.syntax_violation(input, &c);
                        c.advance_unit();
                    } else {
                        self.url.user_start = self.current_position(&c) as u32;
                        state = State::AuthorityOrHost;
                        authority_or_host_begin = c;
                    }
                }
                State::AuthorityOrHost => {
                    // URLParser.cpp 2089, só o laço lento (o caminho rápido com
                    // findHostCharacterOfInterest/scanClass é otimização, pendente).
                    loop {
                        if c.get() == '@' as u32 {
                            let mut last_at = c;
                            let mut find_last_at = c;
                            while !find_last_at.at_end() {
                                if find_last_at.get() == '@' as u32 {
                                    last_at = find_last_at;
                                }
                                let find_char = find_last_at.get();
                                let is_slash =
                                    find_char == '/' as u32 || (self.url_is_special && find_char == '\\' as u32);
                                if is_slash || find_char == '?' as u32 || find_char == '#' as u32 {
                                    break;
                                }
                                find_last_at.advance_unit();
                            }
                            let authority = CodePointIterator::new(
                                &input[..last_at.position()],
                                authority_or_host_begin.position(),
                            );
                            self.parse_authority(input, authority);
                            c = last_at;
                            self.advance(input, &mut c);
                            authority_or_host_begin = c;
                            state = State::Host;
                            self.host_has_percent_or_non_ascii = false;
                            break;
                        }
                        let is_slash = c.get() == '/' as u32 || (self.url_is_special && c.get() == '\\' as u32);
                        if is_slash || c.get() == '?' as u32 || c.get() == '#' as u32 {
                            let iterator = CodePointIterator::new(&input[..c.position()], authority_or_host_begin.position());
                            if iterator.at_end() {
                                if self.url_is_special {
                                    self.failure();
                                    return;
                                }
                                self.url.user_end = self.current_position(&c) as u32;
                                self.url.password_end = self.url.user_end;
                                self.url.host_end = self.url.user_end;
                                self.url.port_length = 0;
                                self.url.path_after_last_slash = self.url.user_end;
                            } else {
                                self.url.user_end = self.current_position(&authority_or_host_begin) as u32;
                                self.url.password_end = self.url.user_end;
                                if self.parse_host_and_port(input, iterator) == HostParsingResult::InvalidHost {
                                    self.failure();
                                    return;
                                }
                                if !is_slash {
                                    if self.url_is_special {
                                        self.syntax_violation(input, &c);
                                        self.append_to_ascii_buffer('/' as u32);
                                    }
                                    self.url.path_after_last_slash = self.current_position(&c) as u32;
                                }
                            }
                            state = State::Path;
                            break;
                        }
                        if is_percent_or_non_ascii(c.get()) {
                            self.host_has_percent_or_non_ascii = true;
                        }
                        c.advance_unit();
                        if c.at_end() {
                            break;
                        }
                    }
                }
                State::Host => {
                    // URLParser.cpp 2212, só o laço lento (o caminho rápido é otimização, pendente).
                    loop {
                        let is_slash = c.get() == '/' as u32 || (self.url_is_special && c.get() == '\\' as u32);
                        if is_slash || c.get() == '?' as u32 || c.get() == '#' as u32 {
                            let host = CodePointIterator::new(&input[..c.position()], authority_or_host_begin.position());
                            if self.parse_host_and_port(input, host) == HostParsingResult::InvalidHost {
                                self.failure();
                                return;
                            }
                            if c.get() == '?' as u32 || c.get() == '#' as u32 {
                                self.syntax_violation(input, &c);
                                self.append_to_ascii_buffer('/' as u32);
                                self.url.path_after_last_slash = self.current_position(&c) as u32;
                            }
                            state = State::Path;
                            break;
                        }
                        if is_percent_or_non_ascii(c.get()) {
                            self.host_has_percent_or_non_ascii = true;
                        }
                        c.advance_unit();
                        if c.at_end() {
                            break;
                        }
                    }
                }
                State::PathStart => {
                    // URLParser.cpp 2506.
                    if c.get() != '/' as u32 && c.get() != '\\' as u32 {
                        self.syntax_violation(input, &c);
                        self.append_to_ascii_buffer('/' as u32);
                    }
                    self.url.path_after_last_slash = self.current_position(&c) as u32;
                    state = State::Path;
                }
                State::Path => {
                    // URLParser.cpp 2514.
                    let mut p = c.position();
                    let mut after_slash = if self.did_see_syntax_violation {
                        self.ascii_buffer.last() == Some(&b'/')
                    } else {
                        p != 0 && input[p - 1] == '/' as u16
                    };
                    debug_assert!(
                        after_slash
                            == (self.current_position(&c) != 0
                                && self.parsed_data_view_at(self.current_position(&c) - 1) == '/' as u16)
                    );
                    loop {
                        debug_assert!(p != input.len());
                        if after_slash && (input[p] == '.' as u16 || input[p] == '%' as u16) {
                            c = iterator_at(p);
                            let is_double_dot = is_double_dot_path_segment(c);
                            if is_double_dot || is_single_dot_path_segment(c) {
                                self.syntax_violation(input, &c);
                                if is_double_dot {
                                    self.consume_double_dot_path_segment(input, &mut c);
                                    self.pop_path();
                                } else {
                                    self.consume_single_dot_path_segment(input, &mut c);
                                }
                                debug_assert!(self.did_see_syntax_violation);
                                after_slash = self.ascii_buffer.last() == Some(&b'/');
                                p = c.position();
                                if p == input.len() {
                                    break;
                                }
                                continue;
                            }
                        }
                        let run_start = p;
                        let (run_end, last_slash) = find_path_run_end(input, p);
                        p = run_end;
                        self.append_span_to_ascii_buffer(&input[run_start..p]);
                        if let Some(last_slash) = last_slash {
                            self.url.path_after_last_slash =
                                (self.current_position(&iterator_at(p)) - (p - last_slash - 1)) as u32;
                        }
                        if p != input.len() && (input[p] == '/' as u16 || (self.url_is_special && input[p] == '\\' as u16)) {
                            if input[p] == '\\' as u16 {
                                self.syntax_violation(input, &iterator_at(p));
                            }
                            self.append_to_ascii_buffer('/' as u32);
                            p += 1;
                            self.url.path_after_last_slash = self.current_position(&iterator_at(p)) as u32;
                            after_slash = true;
                            if p == input.len() {
                                break;
                            }
                            continue;
                        }
                        break;
                    }
                    c = iterator_at(p);
                    if c.at_end() || is_tab_or_newline(c.get()) {
                        continue;
                    }
                    if c.get() == '?' as u32 {
                        self.url.path_end = self.current_position(&c) as u32;
                        self.append_to_ascii_buffer('?' as u32);
                        c.advance_unit();
                        if non_utf8_query_encoding.is_some() {
                            query_begin = c;
                            state = State::NonUtf8Query;
                        } else {
                            state = State::Utf8Query;
                        }
                        continue;
                    }
                    if c.get() == '#' as u32 {
                        self.url.path_end = self.current_position(&c) as u32;
                        self.url.query_end = self.url.path_end;
                        state = State::Fragment;
                        continue;
                    }
                    self.utf8_percent_encode(input, &c, is_in_path_encode_set);
                    c.advance_unit();
                }
                State::OpaquePath => {
                    // URLParser.cpp 2579.
                    let mut start = c.position();
                    let mut p = start;
                    let mut after_last_slash: Option<usize> = None;
                    loop {
                        p = find_opaque_path_stop_character_or_slash(input, p);
                        if p == input.len() || input[p] != '/' as u16 {
                            break;
                        }
                        p += 1;
                        after_last_slash = Some(p);
                    }
                    if let Some(after_last_slash) = after_last_slash {
                        self.append_span_to_ascii_buffer(&input[start..after_last_slash]);
                        self.url.path_after_last_slash = self.current_position(&iterator_at(after_last_slash)) as u32;
                        start = after_last_slash;
                    }
                    self.append_span_to_ascii_buffer(&input[start..p]);
                    c = iterator_at(p);
                    if c.at_end() || is_tab_or_newline(c.get()) {
                        continue;
                    }
                    if c.get() == '?' as u32 {
                        self.url.path_end = self.current_position(&c) as u32;
                        self.append_to_ascii_buffer('?' as u32);
                        c.advance_unit();
                        if non_utf8_query_encoding.is_some() {
                            query_begin = c;
                            state = State::NonUtf8Query;
                        } else {
                            state = State::Utf8Query;
                        }
                    } else if c.get() == '#' as u32 {
                        self.url.path_end = self.current_position(&c) as u32;
                        self.url.query_end = self.url.path_end;
                        state = State::Fragment;
                    } else if c.get() == '/' as u32 {
                        self.append_to_ascii_buffer('/' as u32);
                        c.advance_unit();
                        self.url.path_after_last_slash = self.current_position(&c) as u32;
                    } else if c.get() == ' ' as u32 {
                        let mut next_c = c;
                        advance_without_violation(&mut next_c);
                        debug_assert!(!next_c.at_end());
                        if next_c.get() == '?' as u32 || next_c.get() == '#' as u32 {
                            self.syntax_violation(input, &c);
                            self.percent_encode_byte(b' ');
                        } else {
                            self.append_to_ascii_buffer(' ' as u32);
                        }
                        c.advance_unit();
                    } else {
                        self.utf8_percent_encode(input, &c, is_in_c0_control_encode_set);
                        c.advance_unit();
                    }
                }
                State::File => {
                    // URLParser.cpp 2278.
                    let character = c.get();
                    if character == '\\' as u32 || character == '/' as u32 {
                        if character == '\\' as u32 {
                            self.syntax_violation(input, &c);
                        }
                        self.append_to_ascii_buffer('/' as u32);
                        state = State::FileSlash;
                        c.advance_unit();
                    } else if character == '?' as u32 {
                        self.syntax_violation(input, &c);
                        if base.is_valid() && base.protocol_is_file() {
                            self.copy_url_parts_until(input, base, URLPart::PathEnd, &c, &mut non_utf8_query_encoding);
                            self.append_to_ascii_buffer('?' as u32);
                            c.advance_unit();
                        } else {
                            self.append_span_to_ascii_buffer(&ascii_units(b"///?"));
                            c.advance_unit();
                            self.url.user_start = (self.current_position(&c) - 2) as u32;
                            self.url.user_end = self.url.user_start;
                            self.url.password_end = self.url.user_start;
                            self.url.host_end = self.url.user_start;
                            self.url.port_length = 0;
                            self.url.path_after_last_slash = self.url.user_start + 1;
                            self.url.path_end = self.url.path_after_last_slash;
                        }
                        if non_utf8_query_encoding.is_some() {
                            query_begin = c;
                            state = State::NonUtf8Query;
                        } else {
                            state = State::Utf8Query;
                        }
                    } else if character == '#' as u32 {
                        self.syntax_violation(input, &c);
                        if base.is_valid() && base.protocol_is_file() {
                            self.copy_url_parts_until(input, base, URLPart::QueryEnd, &c, &mut non_utf8_query_encoding);
                            self.append_to_ascii_buffer('#' as u32);
                        } else {
                            self.append_span_to_ascii_buffer(&ascii_units(b"///#"));
                            self.url.user_start = (self.current_position(&c) - 2) as u32;
                            self.url.user_end = self.url.user_start;
                            self.url.password_end = self.url.user_start;
                            self.url.host_end = self.url.user_start;
                            self.url.port_length = 0;
                            self.url.path_after_last_slash = self.url.user_start + 1;
                            self.url.path_end = self.url.path_after_last_slash;
                            self.url.query_end = self.url.path_after_last_slash;
                        }
                        state = State::Fragment;
                        c.advance_unit();
                    } else {
                        self.syntax_violation(input, &c);
                        if base.is_valid() && base.protocol_is_file() && self.should_copy_file_url(input, c) {
                            self.copy_url_parts_until(input, base, URLPart::PathAfterLastSlash, &c, &mut non_utf8_query_encoding);
                        } else {
                            let mut copied_host = false;
                            if base.is_valid() && base.protocol_is_file() {
                                if base.host().is_empty() {
                                    self.copy_url_parts_until(input, base, URLPart::SchemeEnd, &c, &mut non_utf8_query_encoding);
                                    self.append_span_to_ascii_buffer(&ascii_units(b":///"));
                                } else {
                                    self.copy_url_parts_until(input, base, URLPart::PortEnd, &c, &mut non_utf8_query_encoding);
                                    self.append_to_ascii_buffer('/' as u32);
                                    copied_host = true;
                                }
                            } else {
                                self.append_span_to_ascii_buffer(&ascii_units(b"///"));
                            }
                            if !copied_host {
                                self.url.user_start = (self.current_position(&c) - 1) as u32;
                                self.url.user_end = self.url.user_start;
                                self.url.password_end = self.url.user_start;
                                self.url.host_end = self.url.user_start;
                                self.url.port_length = 0;
                            }
                            self.url.path_after_last_slash = self.url.host_end + 1;
                        }
                        if is_windows_drive_letter(c) {
                            self.append_windows_drive_letter(input, &mut c);
                        }
                        state = State::Path;
                    }
                }
                State::FileSlash => {
                    // URLParser.cpp 2363.
                    if c.get() == '/' as u32 || c.get() == '\\' as u32 {
                        if c.get() == '\\' as u32 {
                            self.syntax_violation(input, &c);
                        }
                        if base.is_valid() && base.protocol_is_file() {
                            self.copy_url_parts_until(input, base, URLPart::SchemeEnd, &c, &mut non_utf8_query_encoding);
                            self.append_span_to_ascii_buffer(&ascii_units(b":/"));
                        }
                        self.append_to_ascii_buffer('/' as u32);
                        self.advance(input, &mut c);
                        self.url.user_start = self.current_position(&c) as u32;
                        self.url.user_end = self.url.user_start;
                        self.url.password_end = self.url.user_start;
                        self.url.host_end = self.url.user_start;
                        self.url.port_length = 0;
                        authority_or_host_begin = c;
                        state = State::FileHost;
                    } else {
                        let mut copied_host = false;
                        if base.is_valid() && base.protocol_is_file() {
                            if base.host().is_empty() {
                                self.copy_url_parts_until(input, base, URLPart::SchemeEnd, &c, &mut non_utf8_query_encoding);
                                self.append_span_to_ascii_buffer(&ascii_units(b":///"));
                            } else {
                                self.copy_url_parts_until(input, base, URLPart::PortEnd, &c, &mut non_utf8_query_encoding);
                                self.append_to_ascii_buffer('/' as u32);
                                copied_host = true;
                            }
                        } else {
                            self.syntax_violation(input, &c);
                            self.append_span_to_ascii_buffer(&ascii_units(b"//"));
                        }
                        if !copied_host {
                            self.url.user_start = (self.current_position(&c) - 1) as u32;
                            self.url.user_end = self.url.user_start;
                            self.url.password_end = self.url.user_start;
                            self.url.host_end = self.url.user_start;
                            self.url.port_length = 0;
                        }
                        if is_windows_drive_letter(c) {
                            self.append_windows_drive_letter(input, &mut c);
                            self.url.path_after_last_slash = self.url.host_end + 1;
                        } else if self.copy_base_windows_drive_letter(input, base) {
                            self.append_to_ascii_buffer('/' as u32);
                            self.url.path_after_last_slash = self.url.host_end + 4;
                        } else {
                            self.url.path_after_last_slash = self.url.host_end + 1;
                        }
                        state = State::Path;
                    }
                }
                State::FileHost => {
                    // URLParser.cpp 2416, só o laço lento (o atalho com findHostCharacterOfInterest é otimização, pendente).
                    loop {
                        if is_slash_question_or_hash(c.get()) {
                            let windows_quirk = takes_two_advances_until_end(CodePointIterator::new(
                                &input[..c.position()],
                                authority_or_host_begin.position(),
                            )) && is_windows_drive_letter(authority_or_host_begin);
                            if windows_quirk {
                                self.syntax_violation(input, &authority_or_host_begin);
                                self.append_to_ascii_buffer('/' as u32);
                                self.append_windows_drive_letter(input, &mut authority_or_host_begin);
                            }
                            if windows_quirk || authority_or_host_begin.position() == c.position() {
                                debug_assert!(
                                    windows_quirk
                                        || self.parsed_data_view_at(self.current_position(&c) - 1) == '/' as u16
                                );
                                if c.get() == '?' as u32 {
                                    self.syntax_violation(input, &c);
                                    self.append_span_to_ascii_buffer(&ascii_units(b"/?"));
                                    c.advance_unit();
                                    if non_utf8_query_encoding.is_some() {
                                        query_begin = c;
                                        state = State::NonUtf8Query;
                                    } else {
                                        state = State::Utf8Query;
                                    }
                                    self.url.path_after_last_slash = (self.current_position(&c) - 1) as u32;
                                    self.url.path_end = self.url.path_after_last_slash;
                                    break;
                                }
                                if c.get() == '#' as u32 {
                                    self.syntax_violation(input, &c);
                                    self.append_span_to_ascii_buffer(&ascii_units(b"/#"));
                                    c.advance_unit();
                                    self.url.path_after_last_slash = (self.current_position(&c) - 1) as u32;
                                    self.url.path_end = self.url.path_after_last_slash;
                                    self.url.query_end = self.url.path_after_last_slash;
                                    state = State::Fragment;
                                    break;
                                }
                                state = if authority_or_host_begin.position() == c.position() {
                                    State::FilePathStart
                                } else {
                                    State::Path
                                };
                                break;
                            }
                            let host = CodePointIterator::new(&input[..c.position()], authority_or_host_begin.position());
                            if self.parse_host_and_port(input, host) == HostParsingResult::InvalidHost {
                                self.failure();
                                return;
                            }
                            let password_end = self.url.password_end as usize;
                            if is_localhost(&self.parsed_data_view(password_end, self.current_position(&c) - password_end)) {
                                self.syntax_violation(input, &c);
                                self.ascii_buffer.truncate(password_end);
                                self.url.host_end = self.current_position(&c) as u32;
                                self.url.port_length = 0;
                            }
                            state = State::PathStart;
                            break;
                        }
                        if is_percent_or_non_ascii(c.get()) {
                            self.host_has_percent_or_non_ascii = true;
                        }
                        c.advance_unit();
                        if c.at_end() {
                            break;
                        }
                    }
                }
                State::FilePathStart => {
                    // URLParser.cpp 2491.
                    if c.get() == '/' as u32 || c.get() == '\\' as u32 {
                        if self.url_is_special && c.get() == '\\' as u32 {
                            self.syntax_violation(input, &c);
                        }
                        self.append_to_ascii_buffer('/' as u32);
                        self.advance(input, &mut c);
                        self.url.path_after_last_slash = self.current_position(&c) as u32;
                        if is_windows_drive_letter(c) && self.current_position(&c) == (self.url.host_end + 1) as usize {
                            self.append_windows_drive_letter(input, &mut c);
                        }
                    }
                    state = State::Path;
                }
                State::Utf8Query => {
                    debug_assert!(non_utf8_query_encoding.is_none());
                    if url_query::utf8_query_state(self, input, &mut c) == QueryStateStep::ToFragment {
                        self.url.query_end = self.current_position(&c) as u32;
                        state = State::Fragment;
                    }
                }
                State::NonUtf8Query => {
                    let encoding = non_utf8_query_encoding.expect("NonUTF8Query sem codificação");
                    if url_query::non_utf8_query_state(self, input, &mut c, &query_begin, &mut query_buffer, encoding)
                        == QueryStateStep::ToFragment
                    {
                        self.url.query_end = self.current_position(&c) as u32;
                        state = State::Fragment;
                    }
                }
                State::Fragment => url_query::fragment_state(self, input, &mut c, is_in_fragment_encode_set),
            }
        }

        // Estado final (URLParser.cpp 2682): SchemeStart a SpecialAuthorityIgnoreSlashes.
        match state {
            State::SchemeStart => {
                if self.current_position(&c) == 0 && base.is_valid() && !base.has_opaque_path {
                    self.url = base.clone();
                    self.remove_fragment_identifier();
                    return;
                }
                self.failure();
                return;
            }
            State::Scheme => {
                self.failure();
                return;
            }
            State::NoScheme => unreachable!("RELEASE_ASSERT_NOT_REACHED no C++"),
            State::SpecialRelativeOrAuthority => {
                self.copy_url_parts_until(input, base, URLPart::QueryEnd, &c, &mut non_utf8_query_encoding);
            }
            State::PathOrAuthority => {
                debug_assert!(self.url.user_start != 0);
                debug_assert!(self.url.user_start as usize == self.current_position(&c));
                debug_assert!(self.parsed_data_view_at(self.current_position(&c) - 1) == '/' as u16);
                self.url.user_start -= 1;
                self.url.user_end = self.url.user_start;
                self.url.password_end = self.url.user_start;
                self.url.host_end = self.url.user_start;
                self.url.port_length = 0;
                self.url.path_after_last_slash = self.url.user_start + 1;
                self.url.path_end = self.url.path_after_last_slash;
                self.url.query_end = self.url.path_after_last_slash;
            }
            State::Relative => unreachable!("RELEASE_ASSERT_NOT_REACHED no C++"),
            State::RelativeSlash => {
                self.copy_url_parts_until(input, base, URLPart::PortEnd, &c, &mut non_utf8_query_encoding);
                self.append_to_ascii_buffer('/' as u32);
                self.url.path_after_last_slash = self.url.host_end + self.url.port_length + 1;
                self.url.path_end = self.url.path_after_last_slash;
                self.url.query_end = self.url.path_after_last_slash;
            }
            State::SpecialAuthoritySlashes | State::SpecialAuthorityIgnoreSlashes => {
                self.failure();
                return;
            }
            State::AuthorityOrHost => {
                // URLParser.cpp 2737.
                self.url.user_end = self.current_position(&authority_or_host_begin) as u32;
                self.url.password_end = self.url.user_end;
                if authority_or_host_begin.at_end() {
                    self.url.user_end = self.url.user_start;
                    self.url.password_end = self.url.user_start;
                    self.url.host_end = self.url.user_start;
                    self.url.port_length = 0;
                    self.url.path_end = self.url.user_start;
                } else if self.parse_host_and_port(input, authority_or_host_begin) == HostParsingResult::InvalidHost {
                    self.failure();
                    return;
                } else if self.url_is_special {
                    self.syntax_violation(input, &c);
                    self.append_to_ascii_buffer('/' as u32);
                    self.url.path_end = self.url.host_end + self.url.port_length + 1;
                } else {
                    self.url.path_end = self.url.host_end + self.url.port_length;
                }
                self.url.path_after_last_slash = self.url.path_end;
                self.url.query_end = self.url.path_end;
            }
            State::Host => {
                // URLParser.cpp 2761.
                if self.parse_host_and_port(input, authority_or_host_begin) == HostParsingResult::InvalidHost {
                    self.failure();
                    return;
                }
                if self.url_is_special {
                    self.syntax_violation(input, &c);
                    self.append_to_ascii_buffer('/' as u32);
                    self.url.path_end = self.url.host_end + self.url.port_length + 1;
                } else {
                    self.url.path_end = self.url.host_end + self.url.port_length;
                }
                self.url.path_after_last_slash = self.url.path_end;
                self.url.query_end = self.url.path_end;
            }
            State::File => {
                // URLParser.cpp 2776.
                if base.is_valid() && base.protocol_is_file() {
                    self.copy_url_parts_until(input, base, URLPart::QueryEnd, &c, &mut non_utf8_query_encoding);
                } else {
                    self.syntax_violation(input, &c);
                    self.append_span_to_ascii_buffer(&ascii_units(b"///"));
                    self.url.user_start = (self.current_position(&c) - 1) as u32;
                    self.url.user_end = self.url.user_start;
                    self.url.password_end = self.url.user_start;
                    self.url.host_end = self.url.user_start;
                    self.url.port_length = 0;
                    self.url.path_after_last_slash = self.url.user_start + 1;
                    self.url.path_end = self.url.path_after_last_slash;
                    self.url.query_end = self.url.path_after_last_slash;
                }
            }
            State::FileSlash => {
                // URLParser.cpp 2793.
                self.syntax_violation(input, &c);
                let mut copied_host = false;
                if base.is_valid() && base.protocol_is_file() {
                    if base.host().is_empty() {
                        self.copy_url_parts_until(input, base, URLPart::SchemeEnd, &c, &mut non_utf8_query_encoding);
                        self.append_span_to_ascii_buffer(&ascii_units(b":/"));
                    } else {
                        self.copy_url_parts_until(input, base, URLPart::PortEnd, &c, &mut non_utf8_query_encoding);
                        self.append_to_ascii_buffer('/' as u32);
                        copied_host = true;
                    }
                }
                if !copied_host {
                    self.url.user_start = (self.current_position(&c) + 1) as u32;
                    self.append_span_to_ascii_buffer(&ascii_units(b"//"));
                    self.url.user_end = self.url.user_start;
                    self.url.password_end = self.url.user_start;
                    self.url.host_end = self.url.user_start;
                    self.url.port_length = 0;
                }
                if self.copy_base_windows_drive_letter(input, base) {
                    self.append_to_ascii_buffer('/' as u32);
                    self.url.path_after_last_slash = self.url.host_end + 4;
                } else {
                    self.url.path_after_last_slash = self.url.host_end + 1;
                }
                self.url.path_end = self.url.path_after_last_slash;
                self.url.query_end = self.url.path_after_last_slash;
            }
            State::FileHost => {
                // URLParser.cpp 2825.
                let host = CodePointIterator::new(&input[..c.position()], authority_or_host_begin.position());
                if takes_two_advances_until_end(host) && is_windows_drive_letter(authority_or_host_begin) {
                    self.syntax_violation(input, &authority_or_host_begin);
                    self.append_to_ascii_buffer('/' as u32);
                    self.append_windows_drive_letter(input, &mut authority_or_host_begin);
                    self.url.path_after_last_slash = self.current_position(&c) as u32;
                    self.url.path_end = self.url.path_after_last_slash;
                    self.url.query_end = self.url.path_after_last_slash;
                } else if authority_or_host_begin.position() == c.position() {
                    self.syntax_violation(input, &c);
                    self.append_to_ascii_buffer('/' as u32);
                    self.url.user_start = (self.current_position(&c) - 1) as u32;
                    self.url.user_end = self.url.user_start;
                    self.url.password_end = self.url.user_start;
                    self.url.host_end = self.url.user_start;
                    self.url.port_length = 0;
                    self.url.path_after_last_slash = self.url.user_start + 1;
                    self.url.path_end = self.url.path_after_last_slash;
                    self.url.query_end = self.url.path_after_last_slash;
                } else {
                    if self.parse_host_and_port(input, host) == HostParsingResult::InvalidHost {
                        self.failure();
                        return;
                    }
                    self.syntax_violation(input, &c);
                    let password_end = self.url.password_end as usize;
                    if is_localhost(&self.parsed_data_view(password_end, self.current_position(&c) - password_end)) {
                        self.ascii_buffer.truncate(password_end);
                        self.url.host_end = self.current_position(&c) as u32;
                        self.url.port_length = 0;
                    }
                    self.append_to_ascii_buffer('/' as u32);
                    self.url.path_after_last_slash = self.url.host_end + self.url.port_length + 1;
                    self.url.path_end = self.url.path_after_last_slash;
                    self.url.query_end = self.url.path_after_last_slash;
                }
            }
            State::PathStart => unreachable!("RELEASE_ASSERT_NOT_REACHED no C++ (URLParser.cpp 2820)"),
            State::Path | State::OpaquePath | State::FilePathStart => {
                // URLParser.cpp 2824 e 2829.
                self.url.path_end = self.current_position(&c) as u32;
                self.url.query_end = self.url.path_end;
            }
            State::Utf8Query => {
                self.url.query_end = self.current_position(&c) as u32;
            }
            State::NonUtf8Query => {
                let encoding = non_utf8_query_encoding.expect("NonUTF8Query sem codificação");
                url_query::finish_non_utf8_query(self, input, &c, &query_begin, &query_buffer, encoding);
                self.url.query_end = self.current_position(&c) as u32;
            }
            State::Fragment => {}
        }
        // URLParser.cpp 2899: sem violação de sintaxe a string é a própria entrada; com ela, o buffer ASCII.
        if !self.did_see_syntax_violation {
            self.url.string = self.input_string.clone();
            debug_assert!(self.ascii_buffer.is_empty());
        } else {
            self.url.string = WtfString::from_latin1(&std::mem::take(&mut self.ascii_buffer));
        }
        self.url.is_valid = true;
    }
}

/// Os métodos do `URLParser` que `url_query` usa; os corpos são os do C++ (sem repasse a homônimo inerente).
impl QuerySink for URLParser {
    /// `m_urlIsSpecial`.
    fn url_is_special(&self) -> bool {
        self.url_is_special
    }

    /// `URLParser::syntaxViolation(const CodePointIterator&)` (URLParser.cpp 1432).
    fn syntax_violation(&mut self, input: &[u16], iterator: &CodePointIterator) {
        if !self.did_see_syntax_violation {
            self.begin_syntax_violation(input, iterator);
        }
    }

    /// `URLParser::percentEncodeByte(uint8_t)` (URLParser.cpp 899).
    fn percent_encode_byte(&mut self, byte: u8) {
        debug_assert!(self.did_see_syntax_violation);
        const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
        self.ascii_buffer.extend_from_slice(&[b'%', HEX_UPPER[(byte >> 4) as usize], HEX_UPPER[(byte & 0xF) as usize]]);
    }

    /// `URLParser::appendToASCIIBuffer(char32_t)` (URLParser.cpp 776).
    fn append_to_ascii_buffer(&mut self, code_point: u32) {
        debug_assert!(code_point < 0x80);
        if self.did_see_syntax_violation {
            self.ascii_buffer.push(code_point as u8);
        }
    }

    /// `URLParser::appendToASCIIBuffer(std::span<...>)` (URLParser.cpp 783 e 789).
    fn append_span_to_ascii_buffer(&mut self, characters: &[u16]) {
        if self.did_see_syntax_violation {
            self.ascii_buffer.extend(characters.iter().map(|&unit| unit as u8));
        }
    }

    /// `URLParser::utf8PercentEncode<isInCodeSet>` (URLParser.cpp 908).
    fn utf8_percent_encode(&mut self, input: &[u16], iterator: &CodePointIterator, is_in_code_set: fn(u32) -> bool) {
        debug_assert!(!iterator.at_end());
        let code_point = iterator.get();
        if code_point < 0x80 {
            if is_in_code_set(code_point) {
                self.syntax_violation(input, iterator);
                self.percent_encode_byte(code_point as u8);
            } else {
                self.append_to_ascii_buffer(code_point);
            }
            return;
        }
        debug_assert!(is_in_code_set(code_point), "isInCodeSet should always return true for non-ASCII characters");
        self.syntax_violation(input, iterator);
        // U8_APPEND falha para substituto órfão: o C++ anexa "%EF%BF%BD" (replacementCharacterUTF8PercentEncoded).
        match char::from_u32(code_point) {
            Some(character) => {
                let mut buffer = [0u8; 4];
                for &byte in character.encode_utf8(&mut buffer).as_bytes() {
                    self.percent_encode_byte(byte);
                }
            }
            None => self.ascii_buffer.extend_from_slice(b"%EF%BF%BD"),
        }
    }

    /// `URLParser::advance(CodePointIterator&, const CodePointIterator& forSyntaxViolationPosition)`
    /// com `ReportSyntaxViolation::Yes` (URLParser.cpp 739).
    fn advance_for(&mut self, input: &[u16], iterator: &mut CodePointIterator, for_syntax_violation: &CodePointIterator) {
        iterator.advance_unit();
        while !iterator.at_end() && is_tab_or_newline(iterator.get()) {
            self.syntax_violation(input, for_syntax_violation);
            iterator.advance_unit();
        }
    }
}

// Pendências (a lista completa está no relato da fatia):
//  - atalho "straight-line pass" (1647 a 1801), opcional (só desempenho);
//  - estados AuthorityOrHost .. Fragment (2095 a 2680) e o estado final deles (2745 a 2900);
//  - (feito) finalização de m_url.m_string, parseHostAndPort, parsePort, dnsNameEndsInNumber, needsNonSpecialDotSlash;
//  - (feito) estados File, FileSlash, FileHost, FilePathStart (laço e estado final), sem `todo!` restante;
//    falta só o atalho de desempenho de FileHost (findHostCharacterOfInterest);
//  - a codificação concreta de `URLTextEncoding` fica fora desta árvore (WebCore); aqui só o trait e o sentinela;
//  - URL.cpp restante (setters, hostAndPort, user/password decodificados, file path, etc.).
