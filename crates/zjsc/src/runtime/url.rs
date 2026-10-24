//! `URL` do global. O JavaScriptCore não o define: quem o instala é o bun (WebCore `DOMURL`, binding `JSDOMURL`), como
//! propriedade de dados `writable`, `configurable` e NÃO enumerável. Medido no bun 1.4.2 (`scripts/gen-url-golden.js`,
//! `tests/golden/url_bun.tsv`):
//!
//! - construtor nativo `length` 1, `name` "URL"; chaves próprias `length`, `name`, `prototype`, `parse`(1), `canParse`(1),
//!   `createObjectURL`(1), `revokeObjectURL`(1) (estáticos de dados enumeráveis);
//! - protótipo, nesta ordem: `constructor` (não enumerável), os acessores enumeráveis `href`, `origin` (sem setter),
//!   `protocol`, `username`, `password`, `host`, `hostname`, `port`, `pathname`, `hash`, `search`, `searchParams` (sem
//!   setter), depois `toJSON` e `toString` (dados enumeráveis, `length` 0), `Symbol(nodejs.util.inspect.custom)` e
//!   `@@toStringTag` "URL";
//! - sem `new`: ``Use `new URL(...)` instead of `URL(...)` `` (`ERR_ILLEGAL_CONSTRUCTOR`); sem argumento
//!   `Not enough arguments` (`ERR_MISSING_ARGS`); URL que não analisa: `TypeError` `Invalid URL` (`ERR_INVALID_URL`);
//! - getter com `this` alheio: `The URL.<nome> getter can only be used on instances of URL` (sem `code`); `toString` e
//!   `toJSON`: `Can only call URL.<método> on instances of URL` (`ERR_INVALID_THIS`);
//! - `canParse` e `parse` (`null` se não analisa) convertem o primeiro argumento em texto (símbolo lança); `base` que
//!   não analisa também é `Invalid URL`; `createObjectURL()` sem argumento:
//!   `Not enough arguments to 'createObjectURL'. Expected 1, got 0.`; `revokeObjectURL` devolve `undefined`.
//!
//! A análise é a do `URLParser` portado (`wtf::url_parser`); os getters são os de `DOMURL`. O `searchParams` é um
//! `URLSearchParams` criado na primeira leitura e ligado nos dois sentidos: mudar o objeto reescreve a query do `URL`
//! (`search_params_changed`, chamado pelos métodos que mutam), e mudar o `URL` troca a lista do objeto.
//! Os setters são os de `URLDecomposition` sobre os `URL::set*` portados (`wtf::url_setters`).
//! `createObjectURL(blob)` devolve `blob:<uuid v4>` (sem `nodedata:`, medido) e registra o conteúdo num mapa do programa
//! (zerado em `reset_for_program`); `revokeObjectURL(string)` o tira. Host `xn--` inválido de esquema especial é
//! `Invalid URL` no construtor, na base, no `href` e em `canParse`/`parse` (`hasValidParsedHost`); os setters de host já
//! passam por `hasAcceptableHost`. LACUNAS (relatadas, não inventadas): nada consome o registro ainda (`fetch` da URL); o inspect custom não emite o `Symbol(context)` de `showHidden`.
//! O inspect custom e a origem de `blob:` estão medidos (`gen-url-golden.js`, linhas depois da 664 do TSV).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::blob::{state_of as blob_state_of, BlobState};
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::console_client::is_string_object;
use crate::wtf::weak_random::cryptographically_random_number;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{create_native_class_with_length, install_global_with_attributes, instance_structure, put_native_accessor, throw_coded_type_error};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::symbol::Symbol;
use crate::runtime::url_search_params::{create_search_params, inspect_text, pairs_of_instance, parse_query, quote_units, replace_pairs, serialize};
use crate::runtime::exception_helpers::calculated_class_name;
use crate::runtime::proxy_object::object_get;
use crate::runtime::web_iterable::{put_enumerable_methods, require_arguments, string_value, units_of, usv_units_of};
use crate::wtf::text::string_impl::StringImpl;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url_parser::{URLParser, URL};
use crate::wtf::url_query::QueryEncoding;
use crate::wtf::url_setters::{
    has_valid_punycode_host, decomposition_set_hash, decomposition_set_host, decomposition_set_hostname, decomposition_set_password, decomposition_set_pathname,
    decomposition_set_port, decomposition_set_protocol, decomposition_set_search, decomposition_set_username,
};

type Units = Vec<u16>;

/// O estado de uma instância: a URL analisada e o `searchParams` já criado, se houver.
struct UrlState {
    url: URL,
    search_params: Option<JSValue>,
}

thread_local! {
    /// As instâncias do programa (o valor codificado da célula).
    static URLS: RefCell<HashMap<EncodedJSValue, UrlState>> = RefCell::new(HashMap::new());
    /// De cada `URLSearchParams` criado por um `URL` para o dono.
    static PARAM_OWNERS: RefCell<HashMap<EncodedJSValue, JSValue>> = RefCell::new(HashMap::new());
    /// O protótipo, guardado na instalação, para `URL.parse`.
    static PROTOTYPE: RefCell<Option<JSValue>> = const { RefCell::new(None) };
    /// O registro de `createObjectURL`: de `blob:<uuid>` para o conteúdo do `Blob` naquele instante.
    static OBJECT_URLS: RefCell<HashMap<String, BlobState>> = RefCell::new(HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): o que foi guardado é do programa.
/// As raízes do coletor: o protótipo e os donos dos `searchParams`.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.value(PROTOTYPE.with(|prototype| *prototype.borrow()));
    PARAM_OWNERS.with(|owners| {
        for (params, owner) in owners.borrow().iter() {
            marker.encoded(*params);
            marker.value(*owner);
        }
    });
}

/// O estado de cada `URL` é campo do objeto: o `searchParams` vive com ele.
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    crate::runtime::gc::side_table(&URLS, marker, phase, |state, out| out.extend(state.search_params))
}

pub(crate) fn reset_for_program() {
    let _ = OBJECT_URLS.try_with(|registry| registry.borrow_mut().clear());
    let _ = URLS.try_with(|urls| urls.borrow_mut().clear());
    let _ = PARAM_OWNERS.try_with(|owners| owners.borrow_mut().clear());
}

static PROTOTYPE_S_INFO: ClassInfo = ClassInfo { class_name: "URL", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

pub(crate) fn wtf_units(text: &WtfString) -> Units {
    (0..text.length()).map(|index| text.code_unit_at(index)).collect()
}

fn units_of_str(text: &str) -> Units {
    text.encode_utf16().collect()
}

fn with_url<R>(value: JSValue, body: impl FnOnce(&mut UrlState) -> R) -> Option<R> {
    URLS.with(|urls| urls.borrow_mut().get_mut(&value.encode()).map(body))
}

fn invalid_url(global_object: &JSGlobalObject) -> Thrown {
    throw_coded_type_error(global_object, "Invalid URL", "ERR_INVALID_URL")
}

/// `hasValidParsedHost` (DOMURL.cpp 49): rótulo `xn--` de host de esquema especial que não passa no UTS 46. Só o
/// rótulo literal da entrada precisa de conferência (o que o ICU produz de Unicode é válido por construção): o
/// texto da autoridade de `input` (depois do esquema e das barras, até `/`, `\`, `?` ou `#`) tem `%` ou `xn--`.
fn has_valid_parsed_host(url: &URL, input: &[u16]) -> bool {
    let is_xn_dash_dash = |window: &[u16]| window.iter().zip(b"xn--").all(|(&unit, &expected)| (if (b'A' as u16..=b'Z' as u16).contains(&unit) { unit | 0x20 } else { unit }) == expected as u16);
    let host = wtf_units(&url.host());
    if host.len() < 4 || !host.windows(4).any(is_xn_dash_dash) {
        return true;
    }
    if !url.has_special_scheme() {
        return true;
    }
    if input.iter().any(|&unit| unit == 9 || unit == 10 || unit == 13) {
        return has_valid_punycode_host(&host);
    }
    let is_alpha = |unit: u16| u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_alphabetic());
    let mut start = input.iter().take_while(|&&unit| unit <= 0x20).count();
    if input.get(start).is_some_and(|&unit| is_alpha(unit)) {
        let scheme_end = start + 1 + input[start + 1..].iter().take_while(|&&unit| is_alpha(unit) || u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.'))).count();
        if input.get(scheme_end) == Some(&(b':' as u16)) {
            start = scheme_end + 1;
        }
    }
    start += input[start..].iter().take_while(|&&unit| unit == b'/' as u16 || unit == b'\\' as u16).count();
    let authority = &input[start..];
    let authority = &authority[..authority.iter().position(|&unit| unit < 0x100 && [b'/', b'\\', b'?', b'#'].contains(&(unit as u8))).unwrap_or(authority.len())];
    if !authority.contains(&(b'%' as u16)) && !authority.windows(4).any(is_xn_dash_dash) {
        return true;
    }
    has_valid_punycode_host(&host)
}

/// `URL(input, base)`: `None` se `input` (ou `base`) não analisa ou tem host `xn--` inválido (`DOMURL::create`,
/// `parseBase`, `parseInternal` e `setHref` aplicam `hasValidParsedHost` a cada uma das duas análises).
pub(crate) fn parse_with_base(input: &[u16], base: Option<&[u16]>) -> Option<URL> {
    let base_url = match base {
        Some(base) => {
            let parsed = URLParser::parse_url(&WtfString::from_utf16(base), &URL::default(), QueryEncoding::None);
            if !parsed.is_valid() || !has_valid_parsed_host(&parsed, base) {
                return None;
            }
            parsed
        }
        None => URL::default(),
    };
    let parsed = URLParser::parse_url(&WtfString::from_utf16(input), &base_url, QueryEncoding::None);
    (parsed.is_valid() && has_valid_parsed_host(&parsed, input)).then_some(parsed)
}

/// `(input, base)` dos argumentos 0 e 1, convertidos em texto na ordem; `base` `undefined` conta como ausente.
fn url_arguments(global_object: &JSGlobalObject, call: &HostCall) -> Result<(Units, Option<Units>), Thrown> {
    require_arguments(global_object, call, 1)?;
    let input = usv_units_of(global_object, call.argument(0))?;
    let base = match call.argument(1) {
        base if base.is_undefined() => None,
        base => Some(usv_units_of(global_object, base)?),
    };
    Ok((input, base))
}

fn new_instance(global_object: &JSGlobalObject, structure: &crate::runtime::structure::StructureRef, url: URL) -> JSValue {
    let instance = JSFinalObject::create(global_object.vm(), structure).as_value();
    URLS.with(|urls| urls.borrow_mut().insert(instance.encode(), UrlState { url, search_params: None }));
    instance
}

fn call_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new URL(...)` instead of `URL(...)`", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (input, base) = url_arguments(global_object, call)?;
    let url = parse_with_base(&input, base.as_deref()).ok_or_else(|| invalid_url(global_object))?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    Ok(new_instance(global_object, &structure, url))
}

fn can_parse_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (input, base) = url_arguments(global_object, call)?;
    Ok(JSValue::Bool(parse_with_base(&input, base.as_deref()).is_some()))
}

fn parse_static_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (input, base) = url_arguments(global_object, call)?;
    let Some(url) = parse_with_base(&input, base.as_deref()) else { return Ok(JSValue::null()) };
    Ok(create_url_object(global_object, url))
}

/// Uma instância de `URL` com o protótipo original, como o `DOMURL::create` do C++ (`URL.parse`, `Bun.pathToFileURL`).
pub(crate) fn create_url_object(global_object: &JSGlobalObject, url: URL) -> JSValue {
    let prototype = PROTOTYPE.with(|slot| *slot.borrow()).expect("URL não instalado");
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype);
    new_instance(global_object, &structure, url)
}

/// A URL de uma instância de `URL` (o `WebCoreCast<JSDOMURL>` do C++), ou nada para outro valor.
pub(crate) fn url_of(value: JSValue) -> Option<URL> {
    with_url(value, |state| state.url.clone())
}

/// Um UUID v4 em minúsculas (`WTF::UUID::createVersion4`), dos bytes de `cryptographicallyRandomNumber`.
fn random_uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(4) {
        chunk.copy_from_slice(&cryptographically_random_number().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    let hex: Vec<String> = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}-{}-{}-{}-{}", hex[..4].concat(), hex[4..6].concat(), hex[6..8].concat(), hex[8..10].concat(), hex[10..].concat())
}

/// `URL.createObjectURL(blob)`: `blob:<uuid>` e o registro do conteúdo; o que não é `Blob` (nem `File`) é
/// `ERR_INVALID_ARG_TYPE`.
fn create_object_url_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments to 'createObjectURL'. Expected 1, got 0.", "ERR_MISSING_ARGS"));
    }
    let argument = call.argument(0);
    let state = if argument.is_object() { blob_state_of(argument) } else { None };
    let Some(state) = state else {
        return Err(throw_coded_type_error(global_object, "createObjectURL expects a Blob object", "ERR_INVALID_ARG_TYPE"));
    };
    let url = format!("blob:{}", random_uuid_v4());
    OBJECT_URLS.with(|registry| registry.borrow_mut().insert(url.clone(), state));
    Ok(string_value(global_object, &units_of_str(&url)))
}

/// O conteúdo registrado por `createObjectURL` para `url` (`blob:<uuid>`), se ainda não foi revogado.
pub(crate) fn object_url_state(url: &str) -> Option<BlobState> {
    OBJECT_URLS.with(|registry| registry.borrow().get(url).cloned())
}

/// `URL.revokeObjectURL(url)`: só string (ou `String` encaixotado) vale; tira a URL do registro, existindo ou não.
fn revoke_object_url_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments to 'revokeObjectURL'. Expected 1, got 0.", "ERR_MISSING_ARGS"));
    }
    let argument = call.argument(0);
    if !argument.is_string() && !is_string_object(argument) {
        return Err(throw_coded_type_error(global_object, "revokeObjectURL expects a string", "ERR_INVALID_ARG_TYPE"));
    }
    let url = String::from_utf16_lossy(&units_of(global_object, argument)?);
    OBJECT_URLS.with(|registry| registry.borrow_mut().remove(&url));
    Ok(JSValue::undefined())
}

fn scheme_is(url: &URL, names: &[&str]) -> bool {
    let scheme = wtf_units(&url.protocol());
    names.iter().any(|name| units_of_str(name) == scheme)
}

/// `protocolHostAndPort` (esquema, `://`, host e porta não padrão; `file://` sai sem host).
fn protocol_host_and_port(url: &URL) -> Units {
    let mut out = wtf_units(&url.protocol());
    out.extend(units_of_str("://"));
    out.extend(host_and_port(url));
    out
}

/// `URLDecomposition::origin`: http(s), ftp, ws e wss têm a origem tupla; `blob:` herda a do caminho quando ele é uma
/// dessas ou `file`; os demais são `"null"`.
fn origin_units(url: &URL) -> Units {
    const TUPLE: [&str; 5] = ["http", "https", "ws", "wss", "ftp"];
    if scheme_is(url, &TUPLE) {
        return protocol_host_and_port(url);
    }
    if scheme_is(url, &["blob"]) {
        let inner = URLParser::parse_url(&url.path(), &URL::default(), QueryEncoding::None);
        if inner.is_valid() && (scheme_is(&inner, &TUPLE) || scheme_is(&inner, &["file"])) {
            return protocol_host_and_port(&inner);
        }
    }
    units_of_str("null")
}

fn host_and_port(url: &URL) -> Units {
    let mut out = wtf_units(&url.host());
    if let Some(port) = url.port() {
        out.extend(units_of_str(&format!(":{port}")));
    }
    out
}

fn prefixed(prefix: &str, text: &WtfString) -> Units {
    if text.is_empty() {
        return Units::new();
    }
    let mut out = units_of_str(prefix);
    out.extend(wtf_units(text));
    out
}

/// O corpo de um getter: o texto que `read` tira da URL, ou o `TypeError` de `this` alheio.
fn read_getter(global_object: &JSGlobalObject, call: &HostCall, name: &str, read: fn(&URL) -> Units) -> HostResult {
    match with_url(call.this_value(), |state| read(&state.url)) {
        Some(units) => Ok(string_value(global_object, &units)),
        None => Err(Thrown::type_error(&format!("The URL.{name} getter can only be used on instances of URL"))),
    }
}

macro_rules! url_getter {
    ($function:ident, $body:ident, $name:literal, $read:expr) => {
        fn $body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            read_getter(global_object, call, $name, $read)
        }
        host_function!($function, $body);
    };
}

url_getter!(href_get, href_get_body, "href", |url| wtf_units(url.string()));
url_getter!(origin_get, origin_get_body, "origin", origin_units);
url_getter!(protocol_get, protocol_get_body, "protocol", |url| {
    let mut out = wtf_units(&url.protocol());
    out.push(':' as u16);
    out
});
url_getter!(username_get, username_get_body, "username", |url| wtf_units(&url.encoded_user()));
url_getter!(password_get, password_get_body, "password", |url| wtf_units(&url.encoded_password()));
url_getter!(host_get, host_get_body, "host", host_and_port);
url_getter!(hostname_get, hostname_get_body, "hostname", |url| wtf_units(&url.host()));
url_getter!(port_get, port_get_body, "port", |url| url.port().map_or_else(Units::new, |port| units_of_str(&port.to_string())));
url_getter!(pathname_get, pathname_get_body, "pathname", |url| wtf_units(&url.path()));
url_getter!(search_get, search_get_body, "search", |url| prefixed("?", &url.query()));
url_getter!(hash_get, hash_get_body, "hash", |url| prefixed("#", &url.fragment_identifier()));

/// `searchParams`: criado na primeira leitura, com a query atual.
fn search_params_get_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = call.this_value();
    let Some(existing) = with_url(this_value, |state| state.search_params) else {
        return Err(Thrown::type_error("The URL.searchParams getter can only be used on instances of URL"));
    };
    if let Some(params) = existing {
        return Ok(params);
    }
    let query = with_url(this_value, |state| wtf_units(&state.url.query())).unwrap_or_default();
    let params = create_search_params(global_object, parse_query(&query));
    with_url(this_value, |state| state.search_params = Some(params));
    PARAM_OWNERS.with(|owners| owners.borrow_mut().insert(params.encode(), this_value));
    Ok(params)
}

/// O texto de `href` com a query trocada por `query` (vazia tira o `?`), reanalisado.
fn with_query(url: &URL, query: &str) -> URL {
    let text = wtf_units(url.string());
    let mut out: Units = text[..url.path_end as usize].to_vec();
    if !query.is_empty() {
        out.push('?' as u16);
        out.extend(units_of_str(query));
    }
    if url.has_fragment_identifier() {
        out.extend_from_slice(&text[url.query_end as usize..]);
    }
    URLParser::parse_url(&WtfString::from_utf16(&out), &URL::default(), QueryEncoding::None)
}

/// Chamado pelos métodos de `URLSearchParams` que mutam: se o objeto é o `searchParams` de um `URL`, a query dele
/// passa a ser a serialização da lista.
pub(crate) fn search_params_changed(params: JSValue) {
    let Some(owner) = PARAM_OWNERS.with(|owners| owners.borrow().get(&params.encode()).copied()) else { return };
    let Some(pairs) = crate::runtime::url_search_params::pairs_of_instance(params) else { return };
    let query = serialize(&pairs);
    with_url(owner, |state| {
        let updated = with_query(&state.url, &query);
        if updated.is_valid() {
            state.url = updated;
        }
    });
}

fn this_state_check(call: &HostCall, name: &str) -> Result<(), Thrown> {
    with_url(call.this_value(), |_| ()).ok_or_else(|| Thrown::type_error(&format!("The URL.{name} setter can only be used on instances of URL")))
}

/// `href = value`: reanalisa; não analisando, `Invalid URL`; a lista do `searchParams` acompanha a query nova.
fn href_set_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    this_state_check(call, "href")?;
    require_arguments(global_object, call, 1)?;
    let input = usv_units_of(global_object, call.argument(0))?;
    let url = parse_with_base(&input, None).ok_or_else(|| invalid_url(global_object))?;
    commit_url(call.this_value(), url);
    Ok(JSValue::undefined())
}

/// O `m_url = completeURL` de `DOMURL::setHref`: instala a URL e troca a lista do `searchParams` pela query nova.
fn commit_url(this_value: JSValue, url: URL) {
    let query = wtf_units(&url.query());
    let params = with_url(this_value, |state| {
        state.url = url;
        state.search_params
    })
    .flatten();
    if let Some(params) = params {
        replace_pairs(params, parse_query(&query));
    }
}

/// Os demais setters (`URLDecomposition::set*`): o brand check e a conversão do argumento como o binding; `change`
/// devolve o `URL` que o C++ passaria a `setFullURL` (ou `None` quando o setter sai sem mudar nada). `setFullURL` é
/// `setHref(fullURL.string())`, cuja exceção `URLDecomposition` descarta: o texto é reanalisado e, se não analisa,
/// nada muda.
fn decomposition_setter(global_object: &JSGlobalObject, call: &HostCall, name: &str, change: fn(&URL, &[u16]) -> Option<URL>) -> HostResult {
    this_state_check(call, name)?;
    require_arguments(global_object, call, 1)?;
    let value = usv_units_of(global_object, call.argument(0))?;
    let current = with_url(call.this_value(), |state| state.url.clone()).expect("brand check feito");
    if let Some(updated) = change(&current, &value) {
        if let Some(url) = parse_with_base(&wtf_units(updated.string()), None) {
            commit_url(call.this_value(), url);
        }
    }
    Ok(JSValue::undefined())
}

macro_rules! url_setter {
    ($function:ident, $body:ident, $name:literal, $change:path) => {
        fn $body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            decomposition_setter(global_object, call, $name, $change)
        }
        host_function!($function, $body);
    };
}

url_setter!(protocol_set, protocol_set_body, "protocol", decomposition_set_protocol);
url_setter!(username_set, username_set_body, "username", decomposition_set_username);
url_setter!(password_set, password_set_body, "password", decomposition_set_password);
url_setter!(host_set, host_set_body, "host", decomposition_set_host);
url_setter!(hostname_set, hostname_set_body, "hostname", decomposition_set_hostname);
url_setter!(port_set, port_set_body, "port", decomposition_set_port);
url_setter!(pathname_set, pathname_set_body, "pathname", decomposition_set_pathname);
url_setter!(search_set, search_set_body, "search", decomposition_set_search);
url_setter!(hash_set, hash_set_body, "hash", decomposition_set_hash);

/// `toString` e `toJSON`: o `href`; `this` alheio é `ERR_INVALID_THIS`.
fn href_method(global_object: &JSGlobalObject, call: &HostCall, method: &str) -> HostResult {
    match with_url(call.this_value(), |state| wtf_units(state.url.string())) {
        Some(units) => Ok(string_value(global_object, &units)),
        None => Err(throw_coded_type_error(global_object, &format!("Can only call URL.{method} on instances of URL"), "ERR_INVALID_THIS")),
    }
}

fn to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    href_method(global_object, call, "toString")
}

fn to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    href_method(global_object, call, "toJSON")
}

/// `[Symbol.for('nodejs.util.inspect.custom')](depth, options)` (o `customInspect` do bun): `depth` negativo devolve o
/// próprio `this`; `options.depth` (se dado) menos um é a profundidade dos campos: negativa, `URL [Object]`; zero, o
/// `searchParams` vira `[Object]`. O nome é o do construtor (`URL`, ou o da subclasse). LACUNA: `showHidden` (o
/// `Symbol(context)`) não é emitido.
fn inspect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = call.this_value();
    let Some(url) = with_url(this_value, |state| state.url.clone()) else { return Ok(this_value) };
    let params = search_params_get_body(global_object, call)?;
    let depth_value = call.argument(0);
    let depth = if depth_value.is_symbol() { f64::NAN } else { depth_value.to_number() };
    if depth < 0.0 {
        return Ok(this_value);
    }
    let options = call.argument(1);
    let mut child_depth: Option<f64> = None;
    if options.is_object() {
        let options = options.as_object();
        let name = PropertyName::from_identifier(&Identifier::from_span(global_object.vm(), b"depth"));
        let value = object_get(global_object, &options, &name, options.as_value())?;
        if !value.is_undefined() && !value.is_null() {
            child_depth = Some(if value.is_symbol() { f64::NAN } else { value.to_number() } - 1.0);
        }
    }
    let class_name = String::from_utf16_lossy(&wtf_units(&calculated_class_name(&this_value.as_object())));
    let class_name = if class_name.is_empty() || class_name == "Object" { "URL".to_string() } else { class_name };
    let text = match child_depth {
        Some(child) if child < 0.0 => format!("{class_name} [Object]"),
        _ => {
            let params_text = match child_depth {
                Some(child) if child < 1.0 => "[Object]".to_string(),
                _ => inspect_text(&pairs_of_instance(params).unwrap_or_default()),
            };
            let quoted = |read: fn(&URL) -> Units| quote_units(&read(&url));
            let fields: [(&str, String); 12] = [
                ("href", quoted(|url| wtf_units(url.string()))),
                ("origin", quoted(origin_units)),
                ("protocol", quoted(|url| wtf_units(&url.protocol()).into_iter().chain([':' as u16]).collect())),
                ("username", quoted(|url| wtf_units(&url.encoded_user()))),
                ("password", quoted(|url| wtf_units(&url.encoded_password()))),
                ("host", quoted(host_and_port)),
                ("hostname", quoted(|url| wtf_units(&url.host()))),
                ("port", quoted(|url| url.port().map_or_else(Units::new, |port| units_of_str(&port.to_string())))),
                ("pathname", quoted(|url| wtf_units(&url.path()))),
                ("search", quoted(|url| prefixed("?", &url.query()))),
                ("searchParams", params_text),
                ("hash", quoted(|url| prefixed("#", &url.fragment_identifier()))),
            ];
            let body: Vec<String> = fields.iter().map(|(name, value)| format!("  {name}: {value}")).collect();
            format!("{class_name} {{\n{}\n}}", body.join(",\n"))
        }
    };
    Ok(string_value(global_object, &units_of_str(&text)))
}

host_function!(call_fn, call_body);
host_function!(construct_fn, construct_body);
host_function!(can_parse_fn, can_parse_body);
host_function!(parse_fn, parse_static_body);
host_function!(create_object_url_fn, create_object_url_body);
host_function!(revoke_object_url_fn, revoke_object_url_body);
host_function!(search_params_get, search_params_get_body);
host_function!(href_set, href_set_body);
host_function!(to_string_fn, to_string_body);
host_function!(to_json_fn, to_json_body);
host_function!(inspect_fn, inspect_body);

/// Instala `URL` no global.
pub fn install_url(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let (prototype, constructor) = create_native_class_with_length(global_object, &PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "URL", 1, call_fn, construct_fn);
    PROTOTYPE.with(|slot| *slot.borrow_mut() = Some(prototype.as_value()));
    let statics: [(&str, u32, NativeFunction); 4] =
        [("parse", 1, parse_fn), ("canParse", 1, can_parse_fn), ("createObjectURL", 1, create_object_url_fn), ("revokeObjectURL", 1, revoke_object_url_fn)];
    put_enumerable_methods(global_object, &constructor, &statics);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    let accessors: [(&str, NativeFunction, Option<NativeFunction>); 12] = [
        ("href", href_get, Some(href_set)),
        ("origin", origin_get, None),
        ("protocol", protocol_get, Some(protocol_set)),
        ("username", username_get, Some(username_set)),
        ("password", password_get, Some(password_set)),
        ("host", host_get, Some(host_set)),
        ("hostname", hostname_get, Some(hostname_set)),
        ("port", port_get, Some(port_set)),
        ("pathname", pathname_get, Some(pathname_set)),
        ("hash", hash_get, Some(hash_set)),
        ("search", search_get, Some(search_set)),
        ("searchParams", search_params_get, None),
    ];
    for (name, getter, setter) in accessors {
        put_native_accessor(vm, global_object, &prototype, name, getter, setter, 0);
    }
    let methods: [(&str, u32, NativeFunction); 2] = [("toJSON", 0, to_json_fn), ("toString", 0, to_string_fn)];
    put_enumerable_methods(global_object, &prototype, &methods);
    let inspect_function = JSFunction::create_native(
        vm,
        global_object,
        2,
        &WtfString::from_latin1(b"anonymous"),
        inspect_fn,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    let registered = vm.symbol_registry().symbol_for_key(&StringImpl::create(b"nodejs.util.inspect.custom"));
    let inspect_symbol = Symbol::create_with_registered_uid(vm, &registered);
    prototype.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_private_name(&inspect_symbol.private_name())), inspect_function.as_value(), DONT_ENUM);
    put_to_string_tag(vm, &prototype, "URL");
    install_global_with_attributes(global_object, "URL", constructor.as_value(), DONT_ENUM);
}
