//! Porte dos modificadores de `WTF/wtf/URL.cpp` (`setProtocol`, `setHost`, `setPort`, `setHostAndPort`,
//! `removeHostAndPort`, `setUser`, `setPassword`, `removeCredentials`, `setFragmentIdentifier`,
//! `removeFragmentIdentifier`, `removeQueryAndFragmentIdentifier`, `setQuery`, `setPath`, `remove`) como `impl URL`,
//! mais `URLParser::maybeCanonicalizeScheme` e as regras de `URLDecomposition` (WebCore) que o `DOMURL` usa.
//!
//! O `StringView` do C++ é `&[u16]`; o `StringView` nulo de `setQuery({})` é `None`. Divergências declaradas:
//! `defaultPortForProtocol` não tem o mapa de teste (`setDefaultPortForProtocolForTesting`);
//! `Bun::hasValidPunycodeHost` (NodeURL.cpp 106) não tem o atalho `checkASCIIHostPunycode`: o rótulo `xn--` vai
//! sempre pela conversão UTS 46 completa. Só desempenho: o atalho devolve `NeedsFullCheck` ao que não decide e o
//! veredito dele espelha o do ICU, então o resultado observável é o mesmo (medido: hosts válidos e inválidos no bun).

use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url_host::domain_to_ascii;
use crate::wtf::url_parser::{default_port, is_in_user_info_encode_set, is_special_scheme, scheme_type, URLParser, URL};
use crate::wtf::url_query::QueryEncoding;

/// `HOSTNAME_BUFFER_LENGTH` de `URLParser.h`.
const HOSTNAME_BUFFER_LENGTH: usize = 2048;

fn units(text: &WtfString) -> Vec<u16> {
    (0..text.length()).map(|index| text.code_unit_at(index)).collect()
}

fn ascii(text: &str) -> impl Iterator<Item = u16> + '_ {
    text.bytes().map(u16::from)
}

/// `StringView::substring(start)`: do índice ao fim, vazio se passar do fim.
fn tail(text: &[u16], start: u32) -> &[u16] {
    text.get(start as usize..).unwrap_or(&[])
}

/// `StringView::left(end)`.
fn head(text: &[u16], end: u32) -> &[u16] {
    &text[..(end as usize).min(text.len())]
}

fn is_tab_or_newline(character: u16) -> bool {
    character == 0x09 || character == 0x0A || character == 0x0D
}

fn forward_slash_hash_or_question_mark(character: u16) -> bool {
    character == b'/' as u16 || character == b'#' as u16 || character == b'?' as u16
}

fn slash_hash_or_question_mark(character: u16) -> bool {
    forward_slash_hash_or_question_mark(character) || character == b'\\' as u16
}

fn count_ascii_digits(text: &[u16]) -> usize {
    text.iter().position(|&unit| !(b'0' as u16..=b'9' as u16).contains(&unit)).unwrap_or(text.len())
}

/// `parseInteger<uint16_t>` sobre dígitos ASCII: `None` se vazio, não dígito ou acima de 65535.
fn parse_u16(text: &[u16]) -> Option<u16> {
    if text.is_empty() {
        return None;
    }
    let mut value: u32 = 0;
    for &unit in text {
        if !(b'0' as u16..=b'9' as u16).contains(&unit) {
            return None;
        }
        value = value * 10 + (unit - b'0' as u16) as u32;
        if value > u16::MAX as u32 {
            return None;
        }
    }
    Some(value as u16)
}

fn rfind(text: &[u16], character: char) -> Option<usize> {
    text.iter().rposition(|&unit| unit == character as u16)
}

fn contains(text: &[u16], character: char) -> bool {
    text.contains(&(character as u16))
}

/// `isDefaultPortForProtocol` (URL.cpp 421), sem o mapa de teste.
pub fn is_default_port_for_protocol(port: u16, protocol: &[u16]) -> bool {
    default_port(scheme_type(protocol)) == Some(port as u32)
}

/// `URLParser::maybeCanonicalizeScheme` (URLParser.cpp 1156).
pub fn maybe_canonicalize_scheme(scheme: &[u16]) -> Option<Vec<u16>> {
    if scheme.is_empty() {
        return None;
    }
    let mut index = 0;
    while index < scheme.len() && is_tab_or_newline(scheme[index]) {
        index += 1;
    }
    let is_alpha = |unit: u16| (unit | 0x20) >= b'a' as u16 && (unit | 0x20) <= b'z' as u16;
    if index >= scheme.len() || !is_alpha(scheme[index]) {
        return None;
    }
    index += 1;
    for &unit in &scheme[index..] {
        let alphanumeric = is_alpha(unit) || (b'0' as u16..=b'9' as u16).contains(&unit);
        if alphanumeric || unit == b'+' as u16 || unit == b'-' as u16 || unit == b'.' as u16 || is_tab_or_newline(unit) {
            continue;
        }
        return None;
    }
    Some(
        scheme
            .iter()
            .filter(|&&unit| !is_tab_or_newline(unit))
            .map(|&unit| if (b'A' as u16..=b'Z' as u16).contains(&unit) { unit | 0x20 } else { unit })
            .collect(),
    )
}

/// `appendEncodedHostname` (URL.cpp 496): `None` é o `false` do C++.
fn encoded_hostname(host: &[u16]) -> Option<Vec<u16>> {
    if host.len() > HOSTNAME_BUFFER_LENGTH || host.iter().all(|&unit| unit < 0x80) {
        return Some(host.to_vec());
    }
    let text = String::from_utf16(host).ok()?;
    let converted = domain_to_ascii(&text, true, &mut || {})?;
    Some(converted.into_iter().map(u16::from).collect())
}

/// `percentEncodeCharacters` (URL.cpp 659): codifica em UTF-8 os bytes para os quais `should_encode` diz sim. O `char`
/// com sinal do C++ faz todo byte acima de 0x7F chegar ao predicado como 0xFFxx.
fn percent_encode_characters(input: &[u16], should_encode: impl Fn(u16) -> bool) -> Vec<u16> {
    if !input.iter().any(|&unit| should_encode(unit)) {
        return input.to_vec();
    }
    let text = String::from_utf16_lossy(input);
    let mut out = Vec::new();
    for byte in text.bytes() {
        let seen = if byte >= 0x80 { 0xFF00 | byte as u16 } else { byte as u16 };
        if should_encode(seen) {
            out.extend(ascii(&format!("%{byte:02X}")));
        } else {
            out.push(byte as u16);
        }
    }
    out
}

fn escape_path(path: &[u16]) -> Vec<u16> {
    percent_encode_characters(path, |unit| unit == b'?' as u16 || unit == b'#' as u16 || unit >= 0x80)
}

impl URL {
    /// `URL::hasSpecialScheme()` (URL.cpp 97).
    pub fn has_special_scheme(&self) -> bool {
        self.is_valid && is_special_scheme(&units(&self.protocol()))
    }

    /// `URL::hostAndPort()` (URL.cpp 165).
    pub fn host_and_port(&self) -> WtfString {
        match self.port() {
            Some(port) => {
                let mut out = units(&self.host());
                out.push(b':' as u16);
                out.extend(ascii(&port.to_string()));
                WtfString::from_utf16(&out)
            }
            None => self.host(),
        }
    }

    /// `URL::queryWithLeadingQuestionMark()` (URL.cpp 1240).
    pub fn query_with_leading_question_mark(&self) -> WtfString {
        if self.query_end <= self.path_end {
            return WtfString::default();
        }
        self.string.substring(self.path_end, self.query_end - self.path_end)
    }

    /// `URL::fragmentIdentifierWithLeadingNumberSign()` (URL.cpp 1248).
    pub fn fragment_identifier_with_leading_number_sign(&self) -> WtfString {
        if !self.is_valid || self.string.length() <= self.query_end {
            return WtfString::default();
        }
        self.string.substring(self.query_end, self.string.length() - self.query_end)
    }

    /// `URL::parse(String&&)` (URL.cpp 688).
    fn parse(&mut self, text: &[u16]) {
        *self = URLParser::parse_url(&WtfString::from_utf16(text), &URL::default(), QueryEncoding::None);
    }

    /// `URL::parseAllowingC0AtEnd` (URL.cpp 695).
    fn parse_allowing_c0_at_end(&mut self, text: &[u16]) {
        *self = URLParser::parse_url(&WtfString::from_utf16(text), &URL::default(), QueryEncoding::SentinelAllowingC0AtEnd);
    }

    /// `URL::remove(start, length)` (URL.cpp 702).
    fn remove(&mut self, start: u32, length: u32) {
        if length == 0 {
            return;
        }
        let mut text = units(&self.string);
        let start = (start as usize).min(text.len());
        let end = (start + length as usize).min(text.len());
        text.drain(start..end);
        self.parse(&text);
    }

    /// `URL::setProtocol(StringView)` (URL.cpp 468).
    pub fn set_protocol(&mut self, new_protocol: &[u16]) -> bool {
        let prefix_end = new_protocol.iter().position(|&unit| unit == b':' as u16).unwrap_or(new_protocol.len());
        let Some(canonical) = maybe_canonicalize_scheme(&new_protocol[..prefix_end]) else { return false };
        let text = units(&self.string);

        if !self.is_valid {
            let mut out = canonical;
            out.push(b':' as u16);
            out.extend(text);
            self.parse(&out);
            return true;
        }
        if is_special_scheme(&units(&self.protocol())) != is_special_scheme(&canonical) {
            return true;
        }
        let is_file: Vec<u16> = ascii("file").collect();
        if (self.password_end != self.user_start || self.port().is_some()) && canonical == is_file {
            return true;
        }
        if self.protocol_is_file() && self.host().is_empty() {
            return true;
        }
        let mut out = canonical;
        out.extend_from_slice(tail(&text, self.scheme_end));
        self.parse(&out);
        true
    }

    /// `URL::setHost(StringView)` (URL.cpp 545).
    pub fn set_host(&mut self, new_host: &[u16]) -> bool {
        if !self.is_valid || self.has_opaque_path {
            return false;
        }
        let special = self.has_special_scheme();
        let stop = if special { slash_hash_or_question_mark } else { forward_slash_hash_or_question_mark };
        let new_host = new_host.iter().position(|&unit| stop(unit)).map_or(new_host, |index| &new_host[..index]);
        if contains(new_host, '@') {
            return false;
        }
        if contains(new_host, ':') && new_host.first() != Some(&(b'[' as u16)) {
            return false;
        }
        let encoded = if special {
            let Some(encoded) = encoded_hostname(new_host) else { return false };
            Some(encoded)
        } else {
            None
        };
        let text = units(&self.string);
        let slash_slash_needed = self.user_start == self.scheme_end + 1;
        let mut out = head(&text, self.host_start()).to_vec();
        if slash_slash_needed {
            out.extend(ascii("//"));
        }
        out.extend(encoded.as_deref().unwrap_or(new_host));
        out.extend_from_slice(tail(&text, self.host_end));
        self.parse(&out);
        self.is_valid
    }

    /// `URL::setPort(std::optional<uint16_t>)` (URL.cpp 574).
    pub fn set_port(&mut self, port: Option<u16>) {
        if !self.is_valid {
            return;
        }
        let Some(port) = port else {
            self.remove(self.host_end, self.port_length);
            return;
        };
        let text = units(&self.string);
        let mut out = head(&text, self.host_end).to_vec();
        out.push(b':' as u16);
        out.extend(ascii(&port.to_string()));
        out.extend_from_slice(tail(&text, self.path_start()));
        self.parse(&out);
    }

    /// `URL::setHostAndPort(StringView)` (URL.cpp 602).
    pub fn set_host_and_port(&mut self, host_and_port: &[u16]) {
        if !self.is_valid || self.has_opaque_path {
            return;
        }
        let special = self.has_special_scheme();
        let stop = if special { slash_hash_or_question_mark } else { forward_slash_hash_or_question_mark };
        let host_and_port = host_and_port.iter().position(|&unit| stop(unit)).map_or(host_and_port, |index| &host_and_port[..index]);

        let colon = rfind(host_and_port, ':');
        if colon == Some(0) {
            return;
        }
        let ipv6_separator = rfind(host_and_port, ']');
        let host_only = match (colon, ipv6_separator) {
            (None, _) => true,
            (Some(colon), Some(ipv6)) => ipv6 > colon,
            _ => false,
        };
        if host_only {
            self.set_host(host_and_port);
            return;
        }
        let colon = colon.expect("colon");
        let port_string = &host_and_port[colon + 1..];
        let host_name = &host_and_port[..colon];
        if contains(host_name, '@') {
            return;
        }
        if contains(host_name, ':') && ipv6_separator.is_none() {
            return;
        }
        let port_length = count_ascii_digits(port_string);
        if port_length == 0 {
            self.set_host(host_name);
            return;
        }
        let port_string = &port_string[..port_length];
        let port_string: &[u16] = if parse_u16(port_string).is_some() { port_string } else { &[] };

        let encoded = if special {
            let Some(encoded) = encoded_hostname(host_name) else { return };
            Some(encoded)
        } else {
            None
        };
        let text = units(&self.string);
        let slash_slash_needed = self.user_start == self.scheme_end + 1;
        let mut out = head(&text, self.host_start()).to_vec();
        if slash_slash_needed {
            out.extend(ascii("//"));
        }
        out.extend(encoded.as_deref().unwrap_or(host_name));
        if !port_string.is_empty() {
            out.push(b':' as u16);
        }
        out.extend_from_slice(port_string);
        out.extend_from_slice(tail(&text, self.path_start()));
        self.parse(&out);
    }

    /// `URL::removeHostAndPort()` (URL.cpp 652).
    pub fn remove_host_and_port(&mut self) {
        if self.is_valid {
            self.remove(self.host_start(), self.path_start() - self.host_start());
        }
    }

    /// `URL::setUser(StringView)` (URL.cpp 713).
    pub fn set_user(&mut self, new_user: &[u16]) {
        if !self.is_valid {
            return;
        }
        let text = units(&self.string);
        let mut end = self.user_end;
        let at_end = text.get(end as usize) == Some(&(b'@' as u16));
        if !new_user.is_empty() {
            let slash_slash_needed = self.user_start == self.scheme_end + 1;
            let need_separator = end == self.host_end || (end == self.password_end && !at_end);
            let mut out = head(&text, self.user_start).to_vec();
            if slash_slash_needed {
                out.extend(ascii("//"));
            }
            out.extend(percent_encode_characters(new_user, |unit| is_in_user_info_encode_set(unit as u32)));
            if need_separator {
                out.push(b'@' as u16);
            }
            out.extend_from_slice(tail(&text, end));
            self.parse(&out);
        } else {
            // Remove '@' if we now have neither user nor password.
            if self.user_end == self.password_end && end != self.host_end && at_end {
                end += 1;
            }
            self.remove(self.user_start, end - self.user_start);
        }
    }

    /// `URL::setPassword(StringView)` (URL.cpp 737).
    pub fn set_password(&mut self, new_password: &[u16]) {
        if !self.is_valid {
            return;
        }
        if !new_password.is_empty() {
            let text = units(&self.string);
            let need_leading_slashes = self.user_end == self.scheme_end + 1;
            let mut out = head(&text, self.user_end).to_vec();
            out.extend(ascii(if need_leading_slashes { "//:" } else { ":" }));
            out.extend(percent_encode_characters(new_password, |unit| is_in_user_info_encode_set(unit as u32)));
            out.push(b'@' as u16);
            out.extend_from_slice(tail(&text, self.credentials_end()));
            self.parse(&out);
        } else {
            let end = if self.user_start == self.user_end { self.credentials_end() } else { self.password_end };
            self.remove(self.user_end, end - self.user_end);
        }
    }

    /// `URL::removeCredentials()` (URL.cpp 757).
    pub fn remove_credentials(&mut self) {
        if self.is_valid {
            self.remove(self.user_start, self.credentials_end() - self.user_start);
        }
    }

    /// `URL::setFragmentIdentifier(StringView)` (URL.cpp 765).
    pub fn set_fragment_identifier(&mut self, identifier: &[u16]) {
        if !self.is_valid {
            return;
        }
        let text = units(&self.string);
        let mut out = head(&text, self.query_end).to_vec();
        out.push(b'#' as u16);
        out.extend_from_slice(identifier);
        self.parse_allowing_c0_at_end(&out);
    }

    /// `URL::removeFragmentIdentifier()` (URL.cpp 773).
    pub fn remove_fragment_identifier(&mut self) {
        if self.is_valid {
            self.string = self.string.substring(0, self.query_end);
        }
    }

    /// `URL::removeQueryAndFragmentIdentifier()` (URL.cpp 781).
    pub fn remove_query_and_fragment_identifier(&mut self) {
        if self.is_valid {
            self.string = self.string.substring(0, self.path_end);
            self.query_end = self.path_end;
        }
    }

    /// `URL::setQuery(StringView)` (URL.cpp 790); `None` é o `StringView` nulo.
    pub fn set_query(&mut self, new_query: Option<&[u16]>) {
        if !self.is_valid {
            return;
        }
        let text = units(&self.string);
        let mut out = head(&text, self.path_end).to_vec();
        if let Some(query) = new_query {
            if query.first() != Some(&(b'?' as u16)) {
                out.push(b'?' as u16);
            }
            out.extend_from_slice(query);
        }
        out.extend_from_slice(tail(&text, self.query_end));
        self.parse_allowing_c0_at_end(&out);
    }

    /// `URL::setPath(StringView)` (URL.cpp 840).
    pub fn set_path(&mut self, path: &[u16]) {
        if !self.is_valid {
            return;
        }
        let text = units(&self.string);
        let special = self.has_special_scheme();
        let path_start = self.path_start();
        let starts_with_slash = path.first() == Some(&(b'/' as u16));
        let starts_with_backslash = path.first() == Some(&(b'\\' as u16));
        let no_leading_slash_needed = starts_with_slash
            || (starts_with_backslash && special)
            || (!special && path.is_empty() && self.scheme_end + 1 < path_start);
        let mut out = head(&text, path_start).to_vec();
        if !no_leading_slash_needed {
            out.push(b'/' as u16);
        }
        let slash_slash: Vec<u16> = ascii("//").collect();
        if !special && self.host().is_empty() && path.starts_with(&slash_slash) && path.len() > 2 {
            out.extend(ascii("/."));
        }
        out.extend(escape_path(path));
        out.extend_from_slice(tail(&text, self.path_end));
        self.parse_allowing_c0_at_end(&out);
    }
}

/// `Bun::hasValidPunycodeHost` (NodeURL.cpp 106).
pub fn has_valid_punycode_host(host: &[u16]) -> bool {
    let needle: Vec<u16> = ascii("xn--").collect();
    if !host.windows(needle.len()).any(|window| window == needle.as_slice()) {
        return true;
    }
    use idna::uts46::{AsciiDenyList, DnsLength, Hyphens, Uts46};
    let Ok(text) = String::from_utf16(host) else { return false };
    Uts46::new().to_ascii(text.as_bytes(), AsciiDenyList::EMPTY, Hyphens::Allow, DnsLength::Ignore).is_ok()
}

/// `hasAcceptableHost` (URLDecomposition.cpp 38).
fn has_acceptable_host(url: &URL) -> bool {
    has_valid_punycode_host(&units(&url.host())) || !url.has_special_scheme()
}

/// `URLDecomposition::setProtocol`: o `URL` a instalar com `setFullURL`.
pub fn decomposition_set_protocol(url: &URL, value: &[u16]) -> Option<URL> {
    let mut copy = url.clone();
    copy.set_protocol(value);
    Some(copy)
}

/// `URLDecomposition::setUsername`.
pub fn decomposition_set_username(url: &URL, user: &[u16]) -> Option<URL> {
    if url.host().is_empty() || url.protocol_is_file() {
        return None;
    }
    let mut copy = url.clone();
    copy.set_user(user);
    Some(copy)
}

/// `URLDecomposition::setPassword`.
pub fn decomposition_set_password(url: &URL, password: &[u16]) -> Option<URL> {
    if url.host().is_empty() || url.protocol_is_file() {
        return None;
    }
    let mut copy = url.clone();
    copy.set_password(password);
    Some(copy)
}

/// `URLDecomposition::setHost`. `separator` é o `size_t` do C++: o índice 0 (e só ele) sai sem mudar nada.
pub fn decomposition_set_host(url: &URL, value: &[u16]) -> Option<URL> {
    let mut copy = url.clone();
    if value.is_empty() && !copy.protocol_is_file() && copy.has_special_scheme() {
        return None;
    }
    let separator = rfind(value, ':');
    if separator == Some(0) {
        return None;
    }
    if copy.has_opaque_path {
        return None;
    }
    let ipv6_separator = rfind(value, ']');
    let host_only = match (separator, ipv6_separator) {
        (None, _) => true,
        (Some(separator), Some(ipv6)) => ipv6 > separator,
        _ => false,
    };
    if host_only {
        copy.set_host(value);
    } else {
        let separator = separator.expect("separator");
        // Multiple colons are acceptable only in case of IPv6.
        if value.iter().position(|&unit| unit == b':' as u16) != Some(separator) && ipv6_separator.is_none() {
            return None;
        }
        let port_length = count_ascii_digits(&value[separator + 1..]);
        if port_length == 0 {
            copy.set_host(&value[..separator]);
        } else {
            let port_number = parse_u16(&value[separator + 1..separator + 1 + port_length]);
            let protocol = units(&copy.protocol());
            if port_number.is_some_and(|port| is_default_port_for_protocol(port, &protocol)) {
                copy.set_host_and_port(&value[..separator]);
            } else {
                copy.set_host_and_port(&value[..separator + 1 + port_length]);
            }
        }
    }
    (copy.is_valid() && has_acceptable_host(&copy)).then_some(copy)
}

/// `URLDecomposition::setHostname`.
pub fn decomposition_set_hostname(url: &URL, host: &[u16]) -> Option<URL> {
    let mut copy = url.clone();
    if host.is_empty() && !copy.protocol_is_file() && copy.has_special_scheme() {
        return None;
    }
    if copy.has_opaque_path {
        return None;
    }
    copy.set_host(host);
    (copy.is_valid() && has_acceptable_host(&copy)).then_some(copy)
}

/// `URLDecomposition::parsePort`: o `optional` de fora é "deu para analisar", o de dentro é "sem porta".
pub fn parse_port(string: &[u16], protocol: &[u16]) -> Option<Option<u16>> {
    // https://url.spec.whatwg.org/#port-state with state override given.
    let mut port: u32 = 0;
    let mut found_digit = false;
    for &unit in string {
        if is_tab_or_newline(unit) {
            continue;
        }
        if (b'0' as u16..=b'9' as u16).contains(&unit) {
            port = port * 10 + (unit - b'0' as u16) as u32;
            found_digit = true;
            if port > u16::MAX as u32 {
                return None;
            }
            continue;
        }
        if !found_digit {
            return None;
        }
        break;
    }
    if !found_digit || is_default_port_for_protocol(port as u16, protocol) {
        return Some(None);
    }
    Some(Some(port as u16))
}

/// `URLDecomposition::setPort`.
pub fn decomposition_set_port(url: &URL, value: &[u16]) -> Option<URL> {
    if url.host().is_empty() || url.protocol_is_file() {
        return None;
    }
    let port = parse_port(value, &units(&url.protocol()))?;
    let mut copy = url.clone();
    copy.set_port(port);
    Some(copy)
}

/// `URLDecomposition::setPathname`.
pub fn decomposition_set_pathname(url: &URL, value: &[u16]) -> Option<URL> {
    if url.has_opaque_path {
        return None;
    }
    let mut copy = url.clone();
    copy.set_path(value);
    Some(copy)
}

/// `URLDecomposition::setSearch`: vazio zera a query; o `#` vira `%23` para não vazar no fragmento.
pub fn decomposition_set_search(url: &URL, value: &[u16]) -> Option<URL> {
    let mut copy = url.clone();
    if value.is_empty() {
        copy.set_query(None);
    } else {
        let mut escaped = Vec::with_capacity(value.len());
        for &unit in value {
            if unit == b'#' as u16 {
                escaped.extend(ascii("%23"));
            } else {
                escaped.push(unit);
            }
        }
        copy.set_query(Some(&escaped));
    }
    Some(copy)
}

/// `URLDecomposition::setHash`.
pub fn decomposition_set_hash(url: &URL, value: &[u16]) -> Option<URL> {
    let mut copy = url.clone();
    if value.is_empty() {
        copy.remove_fragment_identifier();
    } else {
        copy.set_fragment_identifier(if value[0] == b'#' as u16 { &value[1..] } else { value });
    }
    Some(copy)
}
