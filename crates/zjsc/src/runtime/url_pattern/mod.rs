//! `URLPattern` do global e do `node:url`: o porte do WebCore do bun 1.4.2 (`URLPattern.cpp`, `URLPatternParser.cpp`,
//! `URLPatternTokenizer.cpp`, `URLPatternConstructorStringParser.cpp`, `URLPatternCanonical.cpp`,
//! `URLPatternComponent.cpp`) e da ligação JS (`JSURLPattern.cpp`, `JSURLPatternInit.cpp`, `JSURLPatternOptions.cpp`,
//! `JSURLPatternResult.cpp`), em `binding`.
//!
//! As cadeias anuláveis do C++ (`String` com `isNull`) são `Option<Units>` aqui: a diferença entre nula e vazia decide o
//! `*` padrão dos componentes e o que entra no `inputs` do resultado.

mod binding;
mod canonical;
mod component;
mod constructor_string_parser;
mod parser;
mod tokenizer;

pub(crate) use binding::{install_url_pattern, reset_for_program, visit_side_table};

use self::canonical::{
    canonicalize_hash, canonicalize_hostname, canonicalize_password, canonicalize_port, canonicalize_protocol, canonicalize_search,
    canonicalize_username, is_absolute_pathname, process_pathname, EncodingCallbackType,
};
use self::component::{ComponentResult, URLPatternComponent};
use self::constructor_string_parser::parse_constructor_string;
use self::parser::{escape_pattern_string, URLPatternStringOptions};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url_parser::{is_special_scheme, URLParser, URL};
use crate::wtf::url_query::QueryEncoding;
use crate::wtf::url_setters::is_default_port_for_protocol;

pub(crate) type Units = Vec<u16>;

pub(crate) fn units(text: &str) -> Units {
    text.encode_utf16().collect()
}

pub(crate) fn wtf(text: &WtfString) -> Units {
    (0..text.length()).map(|index| text.code_unit_at(index)).collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BaseURLStringType {
    Pattern,
    URL,
}

/// `URLPatternInit`.
#[derive(Clone, Default, Debug)]
pub(crate) struct URLPatternInit {
    pub(crate) protocol: Option<Units>,
    pub(crate) username: Option<Units>,
    pub(crate) password: Option<Units>,
    pub(crate) hostname: Option<Units>,
    pub(crate) port: Option<Units>,
    pub(crate) pathname: Option<Units>,
    pub(crate) search: Option<Units>,
    pub(crate) hash: Option<Units>,
    pub(crate) base_url: Option<Units>,
}

/// `URLPattern::URLPatternInput`: `Variant<String, URLPatternInit>`.
#[derive(Clone, Debug)]
pub(crate) enum URLPatternInput {
    String(Units),
    Init(URLPatternInit),
}

/// `URLPatternResult`: as entradas e os oito componentes na ordem protocolo, usuário, senha, host, porta, caminho,
/// busca e fragmento.
pub(crate) struct URLPatternResult {
    pub(crate) inputs: Vec<URLPatternInput>,
    pub(crate) components: [ComponentResult; 8],
}

pub(crate) struct URLPattern {
    protocol: URLPatternComponent,
    username: URLPatternComponent,
    password: URLPatternComponent,
    hostname: URLPatternComponent,
    port: URLPatternComponent,
    pathname: URLPatternComponent,
    search: URLPatternComponent,
    hash: URLPatternComponent,
}

/// https://urlpattern.spec.whatwg.org/#process-a-base-url-string
fn process_base_url_string(input: &[u16], kind: BaseURLStringType) -> Units {
    if kind != BaseURLStringType::Pattern {
        return input.to_vec();
    }
    escape_pattern_string(input)
}

/// https://urlpattern.spec.whatwg.org/#hostname-pattern-is-an-ipv6-address
fn is_hostname_pattern_ipv6(hostname: &[u16]) -> bool {
    if hostname.len() < 2 {
        return false;
    }
    hostname[0] == '[' as u16 || (hostname[0] == '{' as u16 || hostname[0] == '\\' as u16) && hostname[1] == '[' as u16
}

/// `WTF::parseInteger<uint16_t>(text, 10, ParseIntegerWhitespacePolicy::Disallow)`: sinal opcional, dígitos até o
/// fim, estouro (o negativo diferente de zero também) é nada.
fn parse_integer_u16(text: &[u16]) -> Option<u16> {
    let mut rest = text;
    let mut negative = false;
    if rest.first() == Some(&('-' as u16)) {
        negative = true;
        rest = &rest[1..];
    } else if rest.first() == Some(&('+' as u16)) {
        rest = &rest[1..];
    }
    if rest.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    for &unit in rest {
        let digit = (unit as u32).checked_sub('0' as u32).filter(|digit| *digit < 10)? as i64;
        value = value * 10 + if negative { -digit } else { digit };
        if !(0..=u16::MAX as i64).contains(&value) {
            return None;
        }
    }
    Some(value as u16)
}

/// https://urlpattern.spec.whatwg.org/#process-a-urlpatterninit
fn process_init(init: &URLPatternInit, kind: BaseURLStringType) -> Result<URLPatternInit, String> {
    let mut result = URLPatternInit::default();
    let mut base_url: Option<URL> = None;
    if let Some(base) = &init.base_url {
        let parsed = URL::from_string(&WtfString::from_utf16(base));
        if !parsed.is_valid() {
            return Err("Invalid baseURL.".into());
        }
        let no_protocol = init.protocol.is_none();
        let no_hostname = no_protocol && init.hostname.is_none();
        let no_port = no_hostname && init.port.is_none();
        let no_pathname = no_port && init.pathname.is_none();
        let no_search = no_pathname && init.search.is_none();
        if no_protocol {
            result.protocol = Some(process_base_url_string(&wtf(&parsed.protocol()), kind));
        }
        if kind != BaseURLStringType::Pattern && no_port && init.username.is_none() {
            result.username = Some(process_base_url_string(&wtf(&parsed.user()), kind));
        }
        if kind != BaseURLStringType::Pattern && no_port && init.username.is_none() && init.password.is_none() {
            result.password = Some(process_base_url_string(&wtf(&parsed.password()), kind));
        }
        if no_hostname {
            result.hostname = Some(process_base_url_string(&wtf(&parsed.host()), kind));
        }
        if no_port {
            result.port = Some(parsed.port().map_or_else(Units::new, |port| units(&port.to_string())));
        }
        if no_pathname {
            result.pathname = Some(process_base_url_string(&wtf(&parsed.path()), kind));
        }
        if no_search {
            let query = if parsed.has_query() { wtf(&parsed.query()) } else { Units::new() };
            result.search = Some(process_base_url_string(&query, kind));
        }
        if no_search && init.hash.is_none() {
            let fragment = if parsed.has_fragment_identifier() { wtf(&parsed.fragment_identifier()) } else { Units::new() };
            result.hash = Some(process_base_url_string(&fragment, kind));
        }
        base_url = Some(parsed);
    }
    if let Some(protocol) = &init.protocol {
        result.protocol = Some(canonicalize_protocol(protocol, kind)?);
    }
    if let Some(username) = &init.username {
        result.username = Some(canonicalize_username(username, kind));
    }
    if let Some(password) = &init.password {
        result.password = Some(canonicalize_password(password, kind));
    }
    if let Some(hostname) = &init.hostname {
        result.hostname = Some(canonicalize_hostname(hostname, kind)?);
    }
    let result_protocol = result.protocol.clone().unwrap_or_default();
    if let Some(port) = &init.port {
        result.port = Some(canonicalize_port(port, &result_protocol, kind)?);
    }
    if let Some(pathname) = &init.pathname {
        let mut pathname = pathname.clone();
        if let Some(base) = base_url.as_ref().filter(|base| !base.has_opaque_path()) {
            if !is_absolute_pathname(&pathname, kind) {
                let base_path = process_base_url_string(&wtf(&base.path()), kind);
                if let Some(slash_index) = base_path.iter().rposition(|&unit| unit == '/' as u16) {
                    let mut joined = base_path[..=slash_index].to_vec();
                    joined.extend(pathname);
                    pathname = joined;
                }
            }
        }
        result.pathname = Some(process_pathname(&pathname, &result_protocol, kind)?);
    }
    if let Some(search) = &init.search {
        result.search = Some(canonicalize_search(search, kind));
    }
    if let Some(hash) = &init.hash {
        result.hash = Some(canonicalize_hash(hash, kind));
    }
    Ok(result)
}

impl URLPattern {
    /// https://urlpattern.spec.whatwg.org/#url-pattern-create
    pub(crate) fn create(vm: &VM, input: URLPatternInput, base_url: Option<Units>, ignore_case: bool) -> Result<URLPattern, String> {
        let init = match input {
            URLPatternInput::String(text) => {
                let mut init = parse_constructor_string(vm, &text)?;
                if base_url.is_none() && init.protocol.as_ref().is_none_or(|protocol| protocol.is_empty()) {
                    return Err("Relative constructor string must have additional baseURL argument.".into());
                }
                init.base_url = base_url;
                init
            }
            URLPatternInput::Init(init) => {
                if base_url.is_some() {
                    return Err("Constructor with a URLPatternInit should have a null baseURL argument.".into());
                }
                init
            }
        };
        let processed = process_init(&init, BaseURLStringType::Pattern)?;
        let star = || units("*");
        let protocol = processed.protocol.unwrap_or_else(star);
        let username = processed.username.unwrap_or_else(star);
        let password = processed.password.unwrap_or_else(star);
        let hostname = processed.hostname.unwrap_or_else(star);
        let pathname = processed.pathname.unwrap_or_else(star);
        let search = processed.search.unwrap_or_else(star);
        let hash = processed.hash.unwrap_or_else(star);
        let mut port = processed.port.unwrap_or_else(star);
        if let Some(parsed_port) = parse_integer_u16(&port) {
            if is_special_scheme(&protocol) && is_default_port_for_protocol(parsed_port, &protocol) {
                port = Units::new();
            }
        }
        let plain = URLPatternStringOptions::default();
        let protocol_component = URLPatternComponent::compile(vm, &protocol, EncodingCallbackType::Protocol, &plain)?;
        let username_component = URLPatternComponent::compile(vm, &username, EncodingCallbackType::Username, &plain)?;
        let password_component = URLPatternComponent::compile(vm, &password, EncodingCallbackType::Password, &plain)?;
        let hostname_type = if is_hostname_pattern_ipv6(&hostname) { EncodingCallbackType::IPv6Host } else { EncodingCallbackType::Host };
        let hostname_options = URLPatternStringOptions { delimiter_codepoint: units("."), ..Default::default() };
        let hostname_component = URLPatternComponent::compile(vm, &hostname, hostname_type, &hostname_options)?;
        let port_component = URLPatternComponent::compile(vm, &port, EncodingCallbackType::Port, &plain)?;
        let compile_options = URLPatternStringOptions { ignore_case, ..Default::default() };
        let pathname_component = if protocol_component.match_special_scheme_protocol() {
            let options = URLPatternStringOptions { delimiter_codepoint: units("/"), prefix_codepoint: units("/"), ignore_case };
            URLPatternComponent::compile(vm, &pathname, EncodingCallbackType::Path, &options)?
        } else {
            URLPatternComponent::compile(vm, &pathname, EncodingCallbackType::OpaquePath, &compile_options)?
        };
        let search_component = URLPatternComponent::compile(vm, &search, EncodingCallbackType::Search, &compile_options)?;
        let hash_component = URLPatternComponent::compile(vm, &hash, EncodingCallbackType::Hash, &compile_options)?;
        Ok(URLPattern {
            protocol: protocol_component,
            username: username_component,
            password: password_component,
            hostname: hostname_component,
            port: port_component,
            pathname: pathname_component,
            search: search_component,
            hash: hash_component,
        })
    }

    /// Os oito componentes na ordem dos getters: protocolo, usuário, senha, host, porta, caminho, busca e fragmento.
    fn components(&self) -> [&URLPatternComponent; 8] {
        [&self.protocol, &self.username, &self.password, &self.hostname, &self.port, &self.pathname, &self.search, &self.hash]
    }

    /// A string de padrão do componente `index` (na ordem de `components`).
    pub(crate) fn pattern_string(&self, index: usize) -> &Units {
        self.components()[index].pattern_string()
    }

    /// https://urlpattern.spec.whatwg.org/#url-pattern-has-regexp-groups
    pub(crate) fn has_regexp_groups(&self) -> bool {
        self.components().iter().any(|component| component.has_regex_groups_from_part_list())
    }

    /// https://urlpattern.spec.whatwg.org/#url-pattern-match: `Ok(None)` quando não casa (ou a entrada não analisa).
    pub(crate) fn match_input(&self, input: URLPatternInput, base_url_string: Option<Units>) -> Result<Option<URLPatternResult>, String> {
        let mut inputs = vec![input.clone()];
        let values: [Units; 8] = match input {
            URLPatternInput::Init(init) => {
                if base_url_string.is_some() {
                    return Err("Base URL string is provided with a URLPatternInit. If URLPatternInit is provided, please use URLPatternInit.baseURL property instead".into());
                }
                let Ok(processed) = process_init(&init, BaseURLStringType::URL) else { return Ok(None) };
                [processed.protocol, processed.username, processed.password, processed.hostname, processed.port, processed.pathname, processed.search, processed.hash]
                    .map(Option::unwrap_or_default)
            }
            URLPatternInput::String(value) => {
                let mut base_url = URL::default();
                if let Some(base) = &base_url_string {
                    base_url = URL::from_string(&WtfString::from_utf16(base));
                    if !base_url.is_valid() {
                        return Ok(None);
                    }
                    inputs.push(URLPatternInput::String(base.clone()));
                }
                let url = URLParser::parse_url(&WtfString::from_utf16(&value), &base_url, QueryEncoding::None);
                if !url.is_valid() {
                    return Ok(None);
                }
                [
                    wtf(&url.protocol()),
                    wtf(&url.user()),
                    wtf(&url.password()),
                    wtf(&url.host()),
                    url.port().map_or_else(Units::new, |port| units(&port.to_string())),
                    wtf(&url.path()),
                    wtf(&url.query()),
                    wtf(&url.fragment_identifier()),
                ]
            }
        };
        let [protocol, username, password, hostname, port, pathname, search, hash] = values;
        // A ordem do C++: o caminho casa antes da porta.
        let Some(protocol) = self.protocol.component_match(protocol) else { return Ok(None) };
        let Some(username) = self.username.component_match(username) else { return Ok(None) };
        let Some(password) = self.password.component_match(password) else { return Ok(None) };
        let Some(hostname) = self.hostname.component_match(hostname) else { return Ok(None) };
        let Some(pathname) = self.pathname.component_match(pathname) else { return Ok(None) };
        let Some(port) = self.port.component_match(port) else { return Ok(None) };
        let Some(search) = self.search.component_match(search) else { return Ok(None) };
        let Some(hash) = self.hash.component_match(hash) else { return Ok(None) };
        Ok(Some(URLPatternResult { inputs, components: [protocol, username, password, hostname, port, pathname, search, hash] }))
    }
}
