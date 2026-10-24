//! Porte de `WTF/wtf/URLParser.cpp`: hosts IPv4 e IPv6 (`parseIPv4Host`, `parseIPv6Host`,
//! `serializeIPv4`, `serializeIPv6`). Funções livres sobre `&[u16]`; o `url_parser.rs` as chama.
//!
//! O `syntaxViolation(iterator)` do C++ vira o callback `syntax_violation`, chamado nos mesmos
//! pontos; quem chama grava a posição do iterador que ele já tem.

/// `URLParser::IPv4Address` (uint32_t).
pub type IPv4Address = u32;
/// `URLParser::IPv6Address` (`std::array<uint16_t, 8>`).
pub type IPv6Address = [u16; 8];

/// `enum class URLParser::IPv4ParsingError` (URLParser.cpp 3068).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IPv4ParsingError {
    Failure,
    NotIPv4,
}

fn is_tab_or_newline(c: u16) -> bool {
    c == 0x09 || c == 0x0A || c == 0x0D
}

fn is_ascii_digit(c: u16) -> bool {
    (0x30..=0x39).contains(&c)
}

fn is_ascii_hex_digit(c: u16) -> bool {
    is_ascii_digit(c) || ((c | 0x20) >= 0x61 && (c | 0x20) <= 0x66)
}

fn to_ascii_hex_value(c: u16) -> u64 {
    (if is_ascii_digit(c) { c - 0x30 } else { (c | 0x20) - 0x61 + 10 }) as u64
}

/// `parseCanonicalIPv4Address` (URLParser.cpp 433): quatro peças decimais, cada uma até 255 e sem
/// zeros à esquerda.
pub fn parse_canonical_ipv4_address(host: &[u16]) -> Option<u32> {
    let mut p = 0;
    let end = host.len();
    let mut address: u32 = 0;
    let mut piece = 0;
    loop {
        if p == end || !is_ascii_digit(host[p]) {
            return None;
        }
        let mut value = (host[p] - 0x30) as u32;
        p += 1;
        if value != 0 {
            while p != end && is_ascii_digit(host[p]) && value <= 255 {
                value = value * 10 + (host[p] - 0x30) as u32;
                p += 1;
            }
            if value > 255 {
                return None;
            }
        }
        address = address << 8 | value;
        if piece == 3 {
            return if p == end { Some(address) } else { None };
        }
        if p == end || host[p] != 0x2E {
            return None;
        }
        p += 1;
        piece += 1;
    }
}

/// `pow256` (URLParser.cpp 3061).
fn pow256(exponent: usize) -> u64 {
    assert!(exponent <= 4);
    [1u64, 256, 256 * 256, 256 * 256 * 256, 256u64 * 256 * 256 * 256][exponent]
}

/// `URLParser::parseIPv4Host` (URLParser.cpp 3075), https://url.spec.whatwg.org/#concept-ipv4-parser.
/// `syntax_violation` faz o papel de `syntaxViolation(iteratorForSyntaxViolationPosition)`.
pub fn parse_ipv4_host(
    host: &[u16],
    syntax_violation: &mut dyn FnMut(),
) -> Result<IPv4Address, IPv4ParsingError> {
    use IPv4ParsingError::{Failure, NotIPv4};
    let end = host.len();
    let mut p = 0;

    if let Some(address) = parse_canonical_ipv4_address(host) {
        return Ok(address);
    }

    let mut pieces = [0u32; 4];
    let mut piece_count: usize = 0;
    let mut did_see_syntax_violation = false;
    let mut did_see_overflow = false;
    if p != end && host[p] == 0x2E {
        return Err(NotIPv4);
    }
    while p != end {
        if is_tab_or_newline(host[p]) {
            did_see_syntax_violation = true;
            p += 1;
            continue;
        }
        if piece_count >= 4 || host[p] == 0x2E {
            return Err(NotIPv4);
        }

        let mut value: u64 = 0;
        let mut piece_did_overflow = false;
        const MAX_VALUE: u64 = u32::MAX as u64;
        if host[p] == 0x30 {
            p += 1;
            while p != end && is_tab_or_newline(host[p]) {
                did_see_syntax_violation = true;
                p += 1;
            }
            if p != end && host[p] != 0x2E {
                did_see_syntax_violation = true;
                if host[p] == 0x78 || host[p] == 0x58 {
                    p += 1;
                    while p != end {
                        let character = host[p];
                        if is_ascii_hex_digit(character) {
                            value = value * 16 + to_ascii_hex_value(character);
                            if value > MAX_VALUE {
                                piece_did_overflow = true;
                                break;
                            }
                        } else if character == 0x2E {
                            break;
                        } else if is_tab_or_newline(character) {
                            did_see_syntax_violation = true;
                        } else {
                            return Err(NotIPv4);
                        }
                        p += 1;
                    }
                } else {
                    while p != end {
                        let character = host[p];
                        if (0x30..=0x37).contains(&character) {
                            value = value * 8 + (character - 0x30) as u64;
                            if value > MAX_VALUE {
                                piece_did_overflow = true;
                                break;
                            }
                        } else if character == 0x2E {
                            break;
                        } else if is_tab_or_newline(character) {
                            did_see_syntax_violation = true;
                        } else {
                            return Err(NotIPv4);
                        }
                        p += 1;
                    }
                }
            }
        } else {
            while p != end {
                let character = host[p];
                if is_ascii_digit(character) {
                    value = value * 10 + (character - 0x30) as u64;
                    if value > MAX_VALUE {
                        piece_did_overflow = true;
                        break;
                    }
                } else if character == 0x2E {
                    break;
                } else if is_tab_or_newline(character) {
                    did_see_syntax_violation = true;
                } else {
                    return Err(NotIPv4);
                }
                p += 1;
            }
        }
        // On overflow the overflowing digit is left unconsumed and starts a new piece, so what
        // follows can only decide between Failure and NotIPv4.
        if piece_did_overflow {
            did_see_overflow = true;
            pieces[piece_count] = 0;
            piece_count += 1;
            continue;
        }
        pieces[piece_count] = value as u32;
        piece_count += 1;
        if p != end && host[p] == 0x2E {
            p += 1;
            if p == end {
                did_see_syntax_violation = true;
            } else if host[p] == 0x2E {
                return Err(NotIPv4);
            }
        }
    }
    if piece_count == 0 || piece_count > 4 {
        return Err(NotIPv4);
    }
    if did_see_overflow {
        return Err(Failure);
    }
    for i in 0..piece_count - 1 {
        if pieces[i] > 255 {
            return Err(Failure);
        }
    }
    if pieces[piece_count - 1] as u64 >= pow256(5 - piece_count) {
        return Err(Failure);
    }

    if did_see_syntax_violation || piece_count != 4 || pieces[piece_count - 1] > 255 {
        syntax_violation();
    }

    let mut ipv4: u32 = pieces[piece_count - 1];
    for counter in 0..piece_count - 1 {
        ipv4 = ipv4.wrapping_add(pieces[counter].wrapping_mul(pow256(3 - counter) as u32));
    }
    Ok(ipv4)
}

/// `URLParser::parseIPv4PieceInsideIPv6` (URLParser.cpp 3196). Avança `remaining` até o fim da peça.
fn parse_ipv4_piece_inside_ipv6(remaining: &mut &[u16]) -> Option<u32> {
    let end = remaining.len();
    let mut p = 0;
    if p == end {
        return None;
    }
    let mut piece: u32 = 0;
    let mut leading_zeros = false;
    while p != end {
        if !is_ascii_digit(remaining[p]) {
            return None;
        }
        if piece == 0 && remaining[p] == 0x30 {
            if leading_zeros {
                return None;
            }
            leading_zeros = true;
        }
        piece = piece * 10 + (remaining[p] - 0x30) as u32;
        if piece > 255 {
            return None;
        }
        p += 1;
        if p == end {
            break;
        }
        if remaining[p] == 0x2E {
            break;
        }
    }
    *remaining = &remaining[p..];
    if piece != 0 && leading_zeros {
        return None;
    }
    Some(piece)
}

/// `URLParser::parseIPv4AddressInsideIPv6` (URLParser.cpp 3228).
fn parse_ipv4_address_inside_ipv6(mut remaining: &[u16]) -> Option<IPv4Address> {
    let mut address: IPv4Address = 0;
    for i in 0..4 {
        let piece = parse_ipv4_piece_inside_ipv6(&mut remaining)?;
        address = (address << 8) + piece;
        if i < 3 {
            if remaining.is_empty() {
                return None;
            }
            if remaining[0] != 0x2E {
                return None;
            }
            remaining = &remaining[1..];
        } else if !remaining.is_empty() {
            return None;
        }
    }
    Some(address)
}

/// `zeroSequenceLength` (URLParser.cpp 2993).
fn zero_sequence_length(address: &IPv6Address, begin: usize) -> usize {
    let mut end = begin;
    while end < 8 {
        if address[end] != 0 {
            break;
        }
        end += 1;
    }
    end - begin
}

/// `findLongestZeroSequence` (URLParser.cpp 3003).
pub fn find_longest_zero_sequence(address: &IPv6Address) -> Option<usize> {
    let mut longest: Option<usize> = None;
    let mut longest_length = 0;
    let mut i = 0;
    while i < 8 {
        let length = zero_sequence_length(address, i);
        if length != 0 {
            if length > 1 && (longest.is_none() || longest_length < length) {
                longest = Some(i);
                longest_length = length;
            }
            i += length;
        }
        i += 1;
    }
    longest
}

/// `URLParser::parseIPv6Host` (URLParser.cpp 3252), https://url.spec.whatwg.org/#concept-ipv6-parser.
/// `address_characters` é o que está entre os colchetes, sem tabs e newlines.
pub fn parse_ipv6_host(
    address_characters: &[u16],
    syntax_violation: &mut dyn FnMut(),
) -> Option<IPv6Address> {
    let end = address_characters.len();
    let mut p = 0;
    if p == end {
        return None;
    }

    let mut address: IPv6Address = [0; 8];
    let mut piece_pointer: usize = 0;
    let mut compress_pointer: Option<usize> = None;
    let mut previous_value_was_zero = false;
    let mut immediately_after_compress = false;

    if address_characters[p] == 0x3A {
        p += 1;
        if p == end {
            return None;
        }
        if address_characters[p] != 0x3A {
            return None;
        }
        p += 1;
        piece_pointer += 1;
        compress_pointer = Some(piece_pointer);
        immediately_after_compress = true;
    }

    while p != end {
        if piece_pointer == 8 {
            return None;
        }
        if address_characters[p] == 0x3A {
            if compress_pointer.is_some() {
                return None;
            }
            p += 1;
            piece_pointer += 1;
            compress_pointer = Some(piece_pointer);
            immediately_after_compress = true;
            if previous_value_was_zero {
                syntax_violation();
            }
            continue;
        }
        if piece_pointer == 6 || (compress_pointer.is_some() && piece_pointer < 6) {
            if let Some(ipv4) = parse_ipv4_address_inside_ipv6(&address_characters[p..end]) {
                if compress_pointer.is_some() && piece_pointer == 5 {
                    return None;
                }
                syntax_violation();
                address[piece_pointer] = (ipv4 >> 16) as u16;
                piece_pointer += 1;
                address[piece_pointer] = (ipv4 & 0xFFFF) as u16;
                piece_pointer += 1;
                p = end;
                break;
            }
        }
        let mut value: u16 = 0;
        let mut length = 0;
        let mut leading_zeros = false;
        let mut saw_uppercase = false;
        while length < 4 && p != end {
            let character = address_characters[p] as u32;
            let decimal_value = character.wrapping_sub(0x30);
            let letter_value = (character | 0x20).wrapping_sub(0x61);
            let is_decimal = decimal_value < 10;
            let is_hex_letter = letter_value < 6;
            if !is_decimal && !is_hex_letter {
                break;
            }
            saw_uppercase |= is_hex_letter && (character & 0x20) == 0;
            if length == 0 {
                leading_zeros = character == 0x30;
            }
            value = value
                .wrapping_mul(0x10)
                .wrapping_add(if is_decimal { decimal_value } else { letter_value + 10 } as u16);
            length += 1;
            p += 1;
        }
        if saw_uppercase {
            syntax_violation();
        }

        previous_value_was_zero = value == 0;
        if (value != 0 && leading_zeros)
            || (previous_value_was_zero && (length > 1 || immediately_after_compress))
        {
            syntax_violation();
        }

        address[piece_pointer] = value;
        piece_pointer += 1;
        if p == end {
            break;
        }
        if piece_pointer == 8 || address_characters[p] != 0x3A {
            return None;
        }
        p += 1;
        if p == end {
            syntax_violation();
        }

        immediately_after_compress = false;
    }

    if p != end {
        return None;
    }

    if let Some(cp) = compress_pointer {
        let mut swaps = piece_pointer - cp;
        piece_pointer = 7;
        while swaps != 0 {
            let a = piece_pointer;
            piece_pointer = piece_pointer.wrapping_sub(1);
            let b = cp + swaps - 1;
            swaps -= 1;
            address.swap(a, b);
        }
    } else if piece_pointer != 8 {
        return None;
    }

    let mut possible_compress_pointer = find_longest_zero_sequence(&address);
    if let Some(v) = possible_compress_pointer.as_mut() {
        *v += 1;
    }
    if compress_pointer != possible_compress_pointer {
        syntax_violation();
    }

    Some(address)
}

/// `appendNumberToASCIIBuffer<uint8_t>` (URLParser.cpp 2970): decimal sem zeros à esquerda.
fn append_u8(out: &mut Vec<u8>, number: u8) {
    out.extend_from_slice(number.to_string().as_bytes());
}

/// `URLParser::serializeIPv4` (URLParser.cpp 2982).
pub fn serialize_ipv4(address: IPv4Address, out: &mut Vec<u8>) {
    append_u8(out, (address >> 24) as u8);
    out.push(b'.');
    append_u8(out, (address >> 16) as u8);
    out.push(b'.');
    append_u8(out, (address >> 8) as u8);
    out.push(b'.');
    append_u8(out, address as u8);
}

/// `URLParser::serializeIPv6Piece` (URLParser.cpp 3020): hexadecimal minúsculo sem zeros à esquerda.
fn serialize_ipv6_piece(piece: u16, out: &mut Vec<u8>) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut printed = false;
    let nibble0 = piece >> 12;
    if nibble0 != 0 {
        out.push(DIGITS[nibble0 as usize]);
        printed = true;
    }
    let nibble1 = (piece >> 8 & 0xF) as usize;
    if printed || nibble1 != 0 {
        out.push(DIGITS[nibble1]);
        printed = true;
    }
    let nibble2 = (piece >> 4 & 0xF) as usize;
    if printed || nibble2 != 0 {
        out.push(DIGITS[nibble2]);
    }
    out.push(DIGITS[(piece & 0xF) as usize]);
}

/// `URLParser::serializeIPv6` (URLParser.cpp 3038): com colchetes e a compressão do maior bloco de
/// zeros.
pub fn serialize_ipv6(address: &IPv6Address, out: &mut Vec<u8>) {
    out.push(b'[');
    let compress_pointer = find_longest_zero_sequence(address);
    let mut piece = 0;
    while piece < 8 {
        if compress_pointer == Some(piece) {
            debug_assert!(address[piece] == 0);
            if piece != 0 {
                out.push(b':');
            } else {
                out.extend_from_slice(b"::");
            }
            while piece < 8 && address[piece] == 0 {
                piece += 1;
            }
            if piece == 8 {
                break;
            }
        }
        serialize_ipv6_piece(address[piece], out);
        if piece < 7 {
            out.push(b':');
        }
        piece += 1;
    }
    out.push(b']');
}

/// `URLParser::hostnameBufferLength` (URLParser.h 49): o ICU falha com buffer overflow acima disso.
const HOSTNAME_BUFFER_LENGTH: usize = 2048;

/// `URLParser::percentDecodeImpl` (URLParser.cpp 3360). Note o `i + 2 < size` estrito, fiel ao C++:
/// um `%41` no fim exato da entrada não é decodificado. `syntax_violation` é o handler.
pub fn percent_decode(input: &[u8], syntax_violation: &mut dyn FnMut()) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let mut byte = input[i];
        if byte == b'%'
            && i + 2 < input.len()
            && input[i + 1].is_ascii_hexdigit()
            && input[i + 2].is_ascii_hexdigit()
        {
            syntax_violation();
            byte = (to_ascii_hex_value(input[i + 1] as u16) * 16
                + to_ascii_hex_value(input[i + 2] as u16)) as u8;
            i += 2;
        }
        output.push(byte);
        i += 1;
    }
    output
}

/// `URLParser::domainToASCII` (URLParser.cpp 3409). `did_see_syntax_violation` é `m_didSeeSyntaxViolation`.
/// A entrada é `&str` (o chamador já rejeitou surrogate solitário). O caminho ASCII só põe em minúsculas;
/// o resto passa pelo `uidna_nameToASCII` do ICU, aberto com `uidna_openUTS46(CHECK_BIDI | CHECK_CONTEXTJ |
/// NONTRANSITIONAL_TO_ASCII)` (URLParser.cpp 3969), sem `USE_STD3_RULES`. `allowedNameToASCIIErrors`
/// (URLParser.h 39: rótulo vazio, rótulo/domínio longo, hífen inicial/final/3-4) corresponde a
/// `Hyphens::Allow` + `DnsLength::Ignore`; o `idna` aplica CheckBidi e CheckJoiners sempre.
pub fn domain_to_ascii(
    domain: &str,
    did_see_syntax_violation: bool,
    syntax_violation: &mut dyn FnMut(),
) -> Option<Vec<u8>> {
    if domain.is_ascii() {
        let mut saw_uppercase = false;
        let ascii: Vec<u8> = domain
            .bytes()
            .map(|c| {
                saw_uppercase |= c.is_ascii_uppercase();
                c.to_ascii_lowercase()
            })
            .collect();
        if saw_uppercase {
            syntax_violation();
        }
        return Some(ascii);
    }

    use idna::uts46::{AsciiDenyList, DnsLength, Hyphens, Uts46};
    let converted = Uts46::new()
        .to_ascii(domain.as_bytes(), AsciiDenyList::EMPTY, Hyphens::Allow, DnsLength::Ignore)
        .ok()?;
    if converted.is_empty() || converted.len() > HOSTNAME_BUFFER_LENGTH {
        return None;
    }
    let ascii = converted.into_owned().into_bytes();
    if !did_see_syntax_violation && domain.as_bytes() != ascii.as_slice() {
        syntax_violation();
    }
    Some(ascii)
}

/// `URLParser::hasForbiddenHostCodePoint` (URLParser.cpp 3455): algum byte ASCII com a classe
/// `ForbiddenDomain` (o ICU não emite não-ASCII, tab nem newline).
pub fn has_forbidden_host_code_point(ascii_domain: &[u8]) -> bool {
    use crate::wtf::url_character_class_table::{CHARACTER_CLASS_TABLE, FORBIDDEN_DOMAIN};
    ascii_domain
        .iter()
        .any(|&c| c <= 0x7F && CHARACTER_CLASS_TABLE[c as usize] & FORBIDDEN_DOMAIN != 0)
}

/// `hasUnpairedSurrogate` (URLParser.cpp, usada em 3777).
fn has_unpaired_surrogate(host: &[u16]) -> bool {
    char::decode_utf16(host.iter().copied()).any(|r| r.is_err())
}

/// Trecho de `URLParser::parseHostAndPort` (URLParser.cpp 3757 a 3858, incluindo o teste de 3857):
/// de `host` (sem o `:porta`) ao domínio ASCII, ou `None` para `HostParsingResult::InvalidHost`.
/// Cobre os quatro caminhos: sem `%` (3774), `%` só ASCII (3796), `%` com não-ASCII ou tab/newline (3835).
/// `syntax_violation` é `syntaxViolation(hostBegin)`; `did_see_syntax_violation` é `m_didSeeSyntaxViolation`.
/// O que sai depois (IPv4, `lastLabelMayBeANumber`, porta) fica para o `url_parser.rs`.
pub fn parse_host_to_ascii_domain(
    host: &[u16],
    did_see_syntax_violation: &dyn Fn() -> bool,
    syntax_violation: &mut dyn FnMut(),
) -> Option<Vec<u8>> {
    let mut has_percent = false;
    let mut has_tab_or_newline = false;
    let mut has_non_ascii = false;
    for &c in host {
        if c == b'%' as u16 {
            has_percent = true;
        } else if is_tab_or_newline(c) {
            has_tab_or_newline = true;
        } else if c > 0x7F {
            has_non_ascii = true;
        }
    }
    if has_tab_or_newline || has_non_ascii {
        syntax_violation();
    }

    let ascii_domain = if !has_percent {
        // 3774: surrogate solitário é verificado antes de remover tab/newline.
        if has_unpaired_surrogate(host) {
            return None;
        }
        let domain: Vec<u16> = host.iter().copied().filter(|&c| !is_tab_or_newline(c)).collect();
        let domain = String::from_utf16(&domain).ok()?;
        domain_to_ascii(&domain, did_see_syntax_violation(), syntax_violation)
    } else if !has_non_ascii && !has_tab_or_newline {
        // 3796: decodifica, põe em minúsculas e valida numa passada só.
        let mut ascii = Vec::with_capacity(host.len());
        let (mut did_decode, mut saw_uppercase, mut saw_non_ascii_byte) = (false, false, false);
        let mut i = 0;
        while i < host.len() {
            let mut byte = host[i] as u8;
            if byte == b'%'
                && i + 2 < host.len()
                && is_ascii_hex_digit(host[i + 1])
                && is_ascii_hex_digit(host[i + 2])
            {
                byte = (to_ascii_hex_value(host[i + 1]) * 16 + to_ascii_hex_value(host[i + 2])) as u8;
                i += 2;
                did_decode = true;
                saw_non_ascii_byte |= !byte.is_ascii();
            }
            saw_uppercase |= byte.is_ascii_uppercase();
            ascii.push(byte.to_ascii_lowercase());
            i += 1;
        }
        if !saw_non_ascii_byte {
            if did_decode || saw_uppercase {
                syntax_violation();
            }
            Some(ascii)
        } else {
            let utf8_encoded: Vec<u8> = host.iter().map(|&c| c as u8).collect();
            let decoded = percent_decode(&utf8_encoded, syntax_violation);
            let domain = String::from_utf8(decoded).ok()?;
            syntax_violation();
            domain_to_ascii(&domain, did_see_syntax_violation(), syntax_violation)
        }
    } else {
        // 3835: UTF-8 dos code points sem tab/newline (U8_APPEND falha em surrogate solitário).
        let mut utf8_encoded = Vec::with_capacity(host.len());
        for r in char::decode_utf16(host.iter().copied()) {
            let c = r.ok()?;
            if is_tab_or_newline(c as u32 as u16) && (c as u32) < 0x80 {
                continue;
            }
            let mut buf = [0u8; 4];
            utf8_encoded.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        let decoded = percent_decode(&utf8_encoded, syntax_violation);
        if decoded.is_ascii() {
            let domain = String::from_utf8(decoded).ok()?;
            domain_to_ascii(&domain, did_see_syntax_violation(), syntax_violation)
        } else {
            let domain = String::from_utf8(decoded).ok()?;
            syntax_violation();
            domain_to_ascii(&domain, did_see_syntax_violation(), syntax_violation)
        }
    };
    let ascii_domain = ascii_domain?;
    if has_forbidden_host_code_point(&ascii_domain) {
        return None;
    }
    Some(ascii_domain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(s: &str) -> Option<String> {
        let h: Vec<u16> = s.encode_utf16().collect();
        parse_host_to_ascii_domain(&h, &|| false, &mut || {}).map(|v| String::from_utf8(v).unwrap())
    }

    /// Hosts de `tests/golden/url_bun.tsv` (linhas 209 a 219 e 486).
    #[test]
    fn idna_hosts_from_bun_grid() {
        assert_eq!(host("\u{e9}.com").as_deref(), Some("xn--9ca.com"));
        assert_eq!(host("EXAMPLE.C\u{d3}M").as_deref(), Some("example.xn--cm-5ja"));
        assert_eq!(host("b\u{fc}cher.de").as_deref(), Some("xn--bcher-kva.de"));
        assert_eq!(host("xn--bcher-kva.de").as_deref(), Some("xn--bcher-kva.de"));
        assert_eq!(host("\u{65e5}\u{672c}\u{8a9e}.jp").as_deref(), Some("xn--wgv71a119e.jp"));
        // não transicional: o ß fica
        assert_eq!(host("\u{df}.de").as_deref(), Some("xn--zca.de"));
        assert_eq!(host("\u{1c5}.com").as_deref(), Some("xn--d-toa.com"));
        assert_eq!(host("\u{fc}.com").as_deref(), Some("xn--tda.com"));
    }

    #[test]
    fn host_percent_and_forbidden() {
        assert_eq!(host("%C3%BC.com").as_deref(), Some("xn--tda.com"));
        assert_eq!(host("%41bc.COM").as_deref(), Some("abc.com"));
        assert_eq!(host("a\tb.com").as_deref(), Some("ab.com"));
        assert_eq!(host("a b.com"), None);
        assert_eq!(host("a%25b"), None);
        assert_eq!(host("%FF.com"), None);
        assert_eq!(host("\u{fffd}.com"), None);
        assert_eq!(host(""), Some(String::new()));
    }

    #[test]
    fn percent_decode_strict_tail() {
        let mut n = 0;
        assert_eq!(percent_decode(b"a%41b", &mut || n += 1), b"aAb");
        assert_eq!(n, 1);
        // `i + 2 < size` é só a guarda do triplo completo: `%41` colado no fim decodifica (bun:
        // `new URL("http://a%41/").host` é `aa`); `%4` no fim fica como está.
        assert_eq!(percent_decode(b"a%41", &mut || {}), b"aA");
        assert_eq!(percent_decode(b"a%4", &mut || {}), b"a%4");
    }

    #[test]
    fn syntax_violation_flags() {
        let mut n = 0;
        let h: Vec<u16> = "ABC.com".encode_utf16().collect();
        parse_host_to_ascii_domain(&h, &|| false, &mut || n += 1);
        assert_eq!(n, 1);
    }

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn v4(s: &str) -> (Result<u32, IPv4ParsingError>, bool) {
        let mut sv = false;
        let r = parse_ipv4_host(&u(s), &mut || sv = true);
        (r, sv)
    }

    fn v6(s: &str) -> (Option<IPv6Address>, bool) {
        let mut sv = false;
        let r = parse_ipv6_host(&u(s), &mut || sv = true);
        (r, sv)
    }

    fn ser4(a: u32) -> String {
        let mut o = Vec::new();
        serialize_ipv4(a, &mut o);
        String::from_utf8(o).unwrap()
    }

    fn ser6(a: &IPv6Address) -> String {
        let mut o = Vec::new();
        serialize_ipv6(a, &mut o);
        String::from_utf8(o).unwrap()
    }

    #[test]
    fn ipv4_canonical() {
        assert_eq!(v4("127.0.0.1"), (Ok(0x7F000001), false));
        assert_eq!(v4("255.255.255.255"), (Ok(0xFFFFFFFF), false));
    }

    #[test]
    fn ipv4_hex_octal_and_short_forms() {
        assert_eq!(v4("0x7f.1"), (Ok(0x7F000001), true));
        assert_eq!(v4("0177.0.0.1"), (Ok(0x7F000001), true));
        assert_eq!(v4("0x7f000001"), (Ok(0x7F000001), true));
        assert_eq!(v4("2130706433"), (Ok(0x7F000001), true));
        assert_eq!(v4("1.2.3"), (Ok(0x01020003), true));
        assert_eq!(v4("1.2.3."), (Ok(0x01020003), true));
        assert_eq!(v4("0"), (Ok(0), true));
    }

    #[test]
    fn ipv4_tabs_and_newlines() {
        assert_eq!(v4("1.2.3.\t4"), (Ok(0x01020304), true));
    }

    #[test]
    fn ipv4_failures() {
        assert_eq!(v4("256.1.1.1").0, Err(IPv4ParsingError::Failure));
        assert_eq!(v4("4294967296").0, Err(IPv4ParsingError::Failure));
        assert_eq!(v4("1.2.3.4.5").0, Err(IPv4ParsingError::NotIPv4));
        assert_eq!(v4(".1").0, Err(IPv4ParsingError::NotIPv4));
        assert_eq!(v4("1..2").0, Err(IPv4ParsingError::NotIPv4));
        assert_eq!(v4("example.com").0, Err(IPv4ParsingError::NotIPv4));
        assert_eq!(v4("").0, Err(IPv4ParsingError::NotIPv4));
    }

    #[test]
    fn ipv4_serialize() {
        assert_eq!(ser4(0x7F000001), "127.0.0.1");
        assert_eq!(ser4(0), "0.0.0.0");
        assert_eq!(ser4(0xFFFFFFFF), "255.255.255.255");
    }

    #[test]
    fn ipv6_basic() {
        assert_eq!(v6("::1"), (Some([0, 0, 0, 0, 0, 0, 0, 1]), false));
        assert_eq!(v6("::"), (Some([0; 8]), false));
        assert_eq!(
            v6("2001:db8::1"),
            (Some([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]), false)
        );
        assert_eq!(
            v6("1:2:3:4:5:6:7:8"),
            (Some([1, 2, 3, 4, 5, 6, 7, 8]), false)
        );
    }

    #[test]
    fn ipv6_syntax_violations() {
        assert_eq!(v6("2001:DB8::1").1, true);
        assert_eq!(v6("0:0:0:0:0:0:0:1").1, true);
        assert_eq!(v6("::ffff:1.2.3.4"), (Some([0, 0, 0, 0, 0, 0xffff, 0x102, 0x304]), true));
    }

    #[test]
    fn ipv6_failures() {
        assert_eq!(v6("").0, None);
        assert_eq!(v6(":1").0, None);
        assert_eq!(v6("1:2:3:4:5:6:7").0, None);
        assert_eq!(v6("1::2::3").0, None);
        assert_eq!(v6("1:2:3:4:5:6:7:8:9").0, None);
        assert_eq!(v6("12345::").0, None);
        assert_eq!(v6("::1.2.3").0, None);
    }

    #[test]
    fn ipv6_serialize() {
        assert_eq!(ser6(&[0, 0, 0, 0, 0, 0, 0, 1]), "[::1]");
        assert_eq!(ser6(&[0; 8]), "[::]");
        assert_eq!(ser6(&[0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]), "[2001:db8::1]");
        // um bloco único de zeros não é comprimido
        assert_eq!(ser6(&[1, 0, 2, 3, 4, 5, 6, 7]), "[1:0:2:3:4:5:6:7]");
        // o primeiro dos maiores blocos vence
        assert_eq!(ser6(&[1, 0, 0, 2, 0, 0, 3, 4]), "[1::2:0:0:3:4]");
        assert_eq!(ser6(&[1, 0, 0, 0, 2, 0, 0, 0]), "[1::2:0:0:0]");
        assert_eq!(ser6(&[0, 0, 0, 0, 0, 0xffff, 0x102, 0x304]), "[::ffff:102:304]");
    }
}
