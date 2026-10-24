//! `URLPatternCanonical.cpp` (WebCore do bun 1.4.2): as funções de canonização de cada componente
//! (https://urlpattern.spec.whatwg.org/#canonicalization), sobre o `URL` do WTF portado.

use super::{units, wtf, BaseURLStringType, Units};
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url_parser::{is_special_scheme, URL};
use crate::wtf::url_setters::parse_port;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum EncodingCallbackType {
    Protocol,
    Username,
    Password,
    Host,
    IPv6Host,
    Port,
    Path,
    OpaquePath,
    Search,
    Hash,
}

/// O `URL dummyURL("https://w/"_s)` das funções de canonização.
fn dummy_url() -> URL {
    URL::from_string(&WtfString::from_latin1(b"https://w/"))
}

fn parse(text: &[u16]) -> URL {
    URL::from_string(&WtfString::from_utf16(text))
}

fn strip_first(value: &[u16], character: char) -> &[u16] {
    if value.first() == Some(&(character as u16)) { &value[1..] } else { value }
}

/// https://urlpattern.spec.whatwg.org/#is-an-absolute-pathname
pub(crate) fn is_absolute_pathname(input: &[u16], input_type: BaseURLStringType) -> bool {
    if input.is_empty() {
        return false;
    }
    if input[0] == '/' as u16 {
        return true;
    }
    if input_type == BaseURLStringType::URL || input.len() < 2 {
        return false;
    }
    input.starts_with(&units("\\/")) || input.starts_with(&units("{/"))
}

/// https://urlpattern.spec.whatwg.org/#canonicalize-a-protocol (com o process-protocol-for-init).
pub(crate) fn canonicalize_protocol(value: &[u16], value_type: BaseURLStringType) -> Result<Units, String> {
    if value.is_empty() {
        return Ok(value.to_vec());
    }
    let stripped = if value.last() == Some(&(':' as u16)) { &value[..value.len() - 1] } else { value };
    if value_type == BaseURLStringType::Pattern {
        return Ok(stripped.to_vec());
    }
    let mut text = stripped.to_vec();
    text.extend(units("://w/"));
    let dummy = parse(&text);
    if !dummy.is_valid() {
        return Err("Invalid input to canonicalize a URL protocol string.".into());
    }
    Ok(wtf(&dummy.protocol()))
}

/// https://urlpattern.spec.whatwg.org/#canonicalize-a-username (com o process-username-for-init).
pub(crate) fn canonicalize_username(value: &[u16], value_type: BaseURLStringType) -> Units {
    if value.is_empty() || value_type == BaseURLStringType::Pattern {
        return value.to_vec();
    }
    let mut dummy = dummy_url();
    dummy.set_user(value);
    wtf(&dummy.encoded_user())
}

/// https://urlpattern.spec.whatwg.org/#canonicalize-a-password (com o process-password-for-init).
pub(crate) fn canonicalize_password(value: &[u16], value_type: BaseURLStringType) -> Units {
    if value.is_empty() || value_type == BaseURLStringType::Pattern {
        return value.to_vec();
    }
    let mut dummy = dummy_url();
    dummy.set_password(value);
    wtf(&dummy.encoded_password())
}

/// https://urlpattern.spec.whatwg.org/#canonicalize-a-hostname (com o process-hostname-for-init).
pub(crate) fn canonicalize_hostname(value: &[u16], value_type: BaseURLStringType) -> Result<Units, String> {
    if value.is_empty() || value_type == BaseURLStringType::Pattern {
        return Ok(value.to_vec());
    }
    let mut dummy = dummy_url();
    if !dummy.set_host(value) {
        return Err("Invalid input to canonicalize a URL host string.".into());
    }
    Ok(wtf(&dummy.host()))
}

/// https://urlpattern.spec.whatwg.org/#canonicalize-an-ipv6-hostname
pub(crate) fn canonicalize_ipv6_hostname(value: &[u16], value_type: BaseURLStringType) -> Result<Units, String> {
    if value_type == BaseURLStringType::Pattern {
        return Ok(value.to_vec());
    }
    let mut result = Units::with_capacity(value.len());
    for codepoint in char::decode_utf16(value.iter().copied()) {
        // O par substituto vira um ponto de código só (fora do intervalo hexadecimal); o solto também é inválido.
        let Ok(character) = codepoint else {
            return Err("Invalid input to canonicalize a URL IPv6 host string.".into());
        };
        if !(character.is_ascii_hexdigit() || matches!(character, '[' | ']' | ':')) {
            return Err("Invalid input to canonicalize a URL IPv6 host string.".into());
        }
        result.push(character.to_ascii_lowercase() as u16);
    }
    Ok(result)
}

/// https://urlpattern.spec.whatwg.org/#canonicalize-a-port (com o process-port-for-init).
pub(crate) fn canonicalize_port(port_value: &[u16], protocol_value: &[u16], port_value_type: BaseURLStringType) -> Result<Units, String> {
    if port_value.is_empty() || port_value_type == BaseURLStringType::Pattern {
        return Ok(port_value.to_vec());
    }
    match parse_port(port_value, protocol_value) {
        None => Err("Invalid input to canonicalize a URL port string.".into()),
        Some(None) => Ok(Units::new()),
        Some(Some(port)) => Ok(units(&port.to_string())),
    }
}

/// https://urlpattern.spec.whatwg.org/#canonicalize-an-opaque-pathname
pub(crate) fn canonicalize_opaque_pathname(value: &[u16]) -> Result<Units, String> {
    if value.is_empty() {
        return Ok(value.to_vec());
    }
    let mut text = units("a:");
    text.extend_from_slice(value);
    let dummy = parse(&text);
    if !dummy.is_valid() {
        return Err("Invalid input to canonicalize a URL opaque path string.".into());
    }
    Ok(wtf(&dummy.path()))
}

/// https://urlpattern.spec.whatwg.org/#canonicalize-a-pathname
pub(crate) fn canonicalize_pathname(value: &[u16]) -> Units {
    if value.is_empty() {
        return value.to_vec();
    }
    let has_leading_slash = value[0] == '/' as u16;
    let with_prefix = if has_leading_slash {
        value.to_vec()
    } else {
        let mut text = units("/-");
        text.extend_from_slice(value);
        text
    };
    let mut dummy = dummy_url();
    dummy.set_path(&with_prefix);
    let result = wtf(&dummy.path());
    if has_leading_slash { result } else { result.get(2..).unwrap_or_default().to_vec() }
}

/// https://urlpattern.spec.whatwg.org/#process-pathname-for-init
pub(crate) fn process_pathname(value: &[u16], protocol_value: &[u16], value_type: BaseURLStringType) -> Result<Units, String> {
    if value.is_empty() || value_type == BaseURLStringType::Pattern {
        return Ok(value.to_vec());
    }
    if is_special_scheme(protocol_value) || protocol_value.is_empty() {
        return Ok(canonicalize_pathname(value));
    }
    canonicalize_opaque_pathname(value)
}

/// https://urlpattern.spec.whatwg.org/#canonicalize-a-search (com o process-search-for-init).
pub(crate) fn canonicalize_search(value: &[u16], value_type: BaseURLStringType) -> Units {
    if value.is_empty() {
        return value.to_vec();
    }
    let stripped = strip_first(value, '?');
    if value_type == BaseURLStringType::Pattern {
        return stripped.to_vec();
    }
    let mut dummy = dummy_url();
    dummy.set_query(Some(stripped));
    wtf(&dummy.query())
}

/// https://urlpattern.spec.whatwg.org/#canonicalize-a-hash (com o process-hash-for-init).
pub(crate) fn canonicalize_hash(value: &[u16], value_type: BaseURLStringType) -> Units {
    if value.is_empty() {
        return value.to_vec();
    }
    let stripped = strip_first(value, '#');
    if value_type == BaseURLStringType::Pattern {
        return stripped.to_vec();
    }
    let mut dummy = dummy_url();
    dummy.set_fragment_identifier(stripped);
    wtf(&dummy.fragment_identifier())
}

pub(crate) fn call_encoding_callback(kind: EncodingCallbackType, input: &[u16]) -> Result<Units, String> {
    let url = BaseURLStringType::URL;
    match kind {
        EncodingCallbackType::Protocol => canonicalize_protocol(input, url),
        EncodingCallbackType::Username => Ok(canonicalize_username(input, url)),
        EncodingCallbackType::Password => Ok(canonicalize_password(input, url)),
        EncodingCallbackType::Host => canonicalize_hostname(input, url),
        EncodingCallbackType::IPv6Host => canonicalize_ipv6_hostname(input, url),
        EncodingCallbackType::Port => canonicalize_port(input, &[], url),
        EncodingCallbackType::Path => Ok(canonicalize_pathname(input)),
        EncodingCallbackType::OpaquePath => canonicalize_opaque_pathname(input),
        EncodingCallbackType::Search => Ok(canonicalize_search(input, url)),
        EncodingCallbackType::Hash => Ok(canonicalize_hash(input, url)),
    }
}
