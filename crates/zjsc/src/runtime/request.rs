//! `Request` do global, só em memória (sem rede). O bun o instala como propriedade de dados `writable`, `configurable` e
//! ENUMERÁVEL (como `Response`). O corpo e as leituras vêm de `body.rs`, os cabeçalhos de `headers.rs`. Medido no bun 1.4.2
//! (`scripts/gen-fetch-types-golden.js`):
//!
//! - construtor nativo `length` 0, `name` "Request", sem estáticos; protótipo, nesta ordem: `arrayBuffer`, `blob`, `body`,
//!   `bodyUsed`, `bytes`, `cache`, `clone`(1), `credentials`, `destination`, `formData`, `headers`, `integrity`, `json`,
//!   `method`, `mode`, `redirect`, `referrer`, `referrerPolicy`, `signal`, `text`, `textStream`, `url`, e depois `constructor`
//!   (não enumerável) e `@@toStringTag`; não há `keepalive` nem `duplex`;
//! - sem `new`: `Request constructor cannot be invoked without 'new'` (`ERR_ILLEGAL_CONSTRUCTOR`); `this` alheio: igual ao
//!   `Response` (`ERR_INVALID_THIS` num método, texto sem código num acessor);
//! - `new Request(input, init)`: sem argumentos, `Error` `Failed to construct 'Request': 1 argument required, but only 0
//!   present.`; `input` string vazia (ou objeto sem `url` e sem `toString` próprio), `Error` `... url is required.`;
//!   número, `null`, `undefined`: `Error` `... expected non-empty string or object, got undefined`; símbolo: o mesmo sem o
//!   `, got undefined`; objeto: o texto de `input.url` (se existe) senão o `ToString` do objeto; o que não parseia como URL
//!   absoluta (sem base) lança `TypeError` `... Invalid URL "<texto>"` (`ERR_INVALID_URL`); `url` é a forma canônica do
//!   `URLParser`. `input` que é `Request` herda `url`, `method`, cabeçalhos, `redirect`, `cache`, `mode` e uma CÓPIA do corpo
//!   (o original não é marcado como lido);
//! - `init` que não é objeto é ignorado. `method`: `undefined`/`null` não mudam; senão `ToString`, e só vale o nome de uma lista
//!   fechada escrito todo em minúsculas ou todo em maiúsculas (vira maiúsculas), o resto vira `GET` (o `pAtCh` e o `Get`
//!   também). `headers` substitui os herdados; `body` define o corpo (nenhuma restrição por método) e o `Content-Type`
//!   implícito (nunca sobrepõe); `redirect`, `cache` e `mode`: `undefined`/`null` ficam no padrão, não-string lança
//!   `<campo> must be a string`, string fora da lista lança `<campo> must be one of ...` (`ERR_INVALID_ARG_TYPE`);
//!   `credentials`, `referrer`, `referrerPolicy`, `integrity`, `destination` são lidos sem efeito (`include`, vazios);
//!   `signal` ausente/`null` cria um `AbortSignal` novo, um `AbortSignal` é guardado como está, outro valor lança `Failed to
//!   construct 'Request': signal is not of type AbortSignal.`;
//! - `clone` copia o corpo, os cabeçalhos e ganha um `signal` novo; corpo usado lança `Body is disturbed or locked`.
//!
//! O `body` é o `ReadableStream` de `body.rs` (o do usuário, quando `body: stream`); `textStream` devolve um `ReadableStream` de texto (`body::text_stream_of`).
//! O estado das instâncias fica em `thread_local` (zerado em `reset_for_program`), como `Response`.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::abort_signal::{is_signal, new_signal};
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::body::{clone_body, consume_kind, extract, stream_property, text_stream_of, Body, Extracted, ReadKind};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::console_client::is_string_object;
use crate::runtime::headers::{append_default_header, clone_headers, headers_is_empty, make_headers};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::describe_received;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::operations::same_value;
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::json_object::is_symbol;
use crate::runtime::native_class_support::{create_native_subclass, install_global_with_attributes, instance_structure};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::node_error::{throw_coded_type_error, throw_plain_error};
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM};
use crate::runtime::property_name::PropertyName;
use crate::runtime::response::property_of;
use crate::runtime::web_iterable::{string_value, units_of};
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url::URL;

/// Os métodos que o bun aceita (nomes do `llhttp` que ele reconhece), em minúsculas.
const METHODS: &[&str] = &[
    "get", "head", "post", "put", "delete", "options", "patch", "connect", "trace", "link", "unlink", "propfind", "proppatch", "mkcol", "copy", "move", "lock", "unlock",
    "report", "search", "merge", "mkactivity", "checkout", "notify", "subscribe", "unsubscribe", "purge", "source", "query", "m-search", "bind", "rebind", "unbind", "acl",
    "mkcalendar",
];
const REDIRECTS: &[&str] = &["follow", "manual", "error"];
const CACHES: &[&str] = &["default", "no-store", "reload", "no-cache", "force-cache", "only-if-cached"];
const MODES: &[&str] = &["same-origin", "no-cors", "cors", "navigate"];

#[derive(Clone)]
pub(crate) struct RequestState {
    pub(crate) method: String,
    pub(crate) url: Vec<u16>,
    /// O objeto `Headers` desta requisição (o mesmo a cada leitura de `headers`).
    pub(crate) headers: JSValue,
    pub(crate) body: Option<Body>,
    redirect: &'static str,
    cache: &'static str,
    mode: &'static str,
    signal: JSValue,
}

static PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Request", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

thread_local! {
    /// As instâncias do programa (o valor codificado da célula) com o estado.
    static INSTANCES: RefCell<HashMap<EncodedJSValue, RequestState>> = RefCell::new(HashMap::new());
    /// O protótipo de `Request`, guardado na instalação para as instâncias que `clone` cria.
    static REQUEST_PROTOTYPE: RefCell<Option<JSValue>> = const { RefCell::new(None) };
}

/// Fim do programa (`cell_registry::reset_program_state`): o que foi guardado é do programa.
/// As raízes do coletor: o protótipo; o estado dos pedidos guarda valores e adia a coleta enquanto existir.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.value(REQUEST_PROTOTYPE.with(|prototype| *prototype.borrow()));
}

/// O estado de cada `Request` é campo do objeto: `headers`, `signal` e o stream do corpo vivem com ele.
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    crate::runtime::gc::side_table(&INSTANCES, marker, phase, |state, out| {
        out.extend([state.headers, state.signal]);
        out.extend(state.body.as_ref().and_then(|body| body.stream));
    })
}

pub(crate) fn reset_for_program() {
    let _ = INSTANCES.try_with(|instances| instances.borrow_mut().clear());
}

pub(crate) fn state_of(value: JSValue) -> Option<RequestState> {
    INSTANCES.with(|instances| instances.borrow().get(&value.encode()).cloned())
}

/// O estado de `this` para um método; senão o `TypeError` de `this` inválido.
fn method_state(global_object: &JSGlobalObject, call: &HostCall) -> Result<RequestState, Thrown> {
    state_of(call.this_value()).ok_or_else(|| {
        let received = describe_received(global_object, call.this_value()).map(|text| format!(", but received {text}")).unwrap_or_default();
        throw_coded_type_error(global_object, &format!("Expected this to be instanceof Request{received}"), "ERR_INVALID_THIS")
    })
}

/// O estado de `this` para um acessor; senão o `TypeError` sem código.
fn accessor_state(call: &HostCall, name: &str) -> Result<RequestState, Thrown> {
    state_of(call.this_value()).ok_or_else(|| Thrown::type_error(&format!("The Request.{name} getter can only be used on instances of Request")))
}

fn call_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Request constructor cannot be invoked without 'new'", "ERR_ILLEGAL_CONSTRUCTOR"))
}

/// O erro do construtor, sempre com o prefixo `Failed to construct 'Request': `.
fn construct_error(global_object: &JSGlobalObject, message: &str) -> Thrown {
    throw_plain_error(global_object, &format!("Failed to construct 'Request': {message}"))
}

/// O texto de `input` quando não é uma `Request`: string, ou objeto (`url`, senão o texto do objeto).
fn input_units(global_object: &JSGlobalObject, input: JSValue) -> Result<Vec<u16>, Thrown> {
    if input.is_string() {
        return units_of(global_object, input);
    }
    if input.is_object() {
        // Request.rs: `url` presente (qualquer valor, até `null`) vale convertido a string; senão só um `toString`
        // chamável conta (`implementsToString`), e o `Object.prototype.toString` de um objeto não-proxy não conta
        // (medido: `{}` dá "url is required.", `new Proxy({}, {})` dá `Invalid URL "[object Object]"`).
        let url = property_of(global_object, input, b"url")?;
        if !url.is_undefined() {
            return units_of(global_object, url);
        }
        let to_string = property_of(global_object, input, b"toString")?;
        let default_to_string = property_of(global_object, global_object.object_prototype().as_value(), b"toString")?;
        let is_proxy = input.as_object().type_() == JSType::ProxyObjectType;
        if to_string.is_callable() && (is_proxy || !same_value(to_string, default_to_string)) {
            return units_of(global_object, input);
        }
        return Ok(Vec::new());
    }
    if is_symbol(input) {
        return Err(construct_error(global_object, "expected non-empty string or object"));
    }
    Err(construct_error(global_object, "expected non-empty string or object, got undefined"))
}

/// A forma canônica de `units` como URL absoluta; vazio e o que não parseia lançam.
fn canonical_url(global_object: &JSGlobalObject, units: &[u16]) -> Result<Vec<u16>, Thrown> {
    if units.is_empty() {
        return Err(construct_error(global_object, "url is required."));
    }
    // O parser WHATWG do `new URL` (medido: esquema e host minúsculos, porta padrão some).
    let Some(parsed) = crate::runtime::url::parse_with_base(units, None) else {
        let message = format!("Failed to construct 'Request': Invalid URL \"{}\"", String::from_utf16_lossy(units));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_URL"));
    };
    let text = parsed.string();
    Ok((0..text.length()).map(|index| text.code_unit_at(index)).collect())
}

/// `method` normalizado: só o nome da lista escrito todo em minúsculas ou todo em maiúsculas vale, o resto é `GET`.
fn normalized_method(units: &[u16]) -> String {
    let text = String::from_utf16_lossy(units);
    let lower = text.to_ascii_lowercase();
    if (text == lower || text == text.to_ascii_uppercase()) && METHODS.contains(&lower.as_str()) {
        lower.to_ascii_uppercase()
    } else {
        "GET".to_string()
    }
}

/// `redirect`, `cache` ou `mode` de `init`: `None` se ausente, senão o valor da lista `allowed`.
fn choice(global_object: &JSGlobalObject, init: JSValue, name: &str, allowed: &[&'static str], expectation: &str) -> Result<Option<&'static str>, Thrown> {
    let value = property_of(global_object, init, name.as_bytes())?;
    if value.is_undefined_or_null() {
        return Ok(None);
    }
    if !value.is_string() && !is_string_object(value) {
        return Err(throw_coded_type_error(global_object, &format!("{name} must be a string"), "ERR_INVALID_ARG_TYPE"));
    }
    let text = String::from_utf16_lossy(&units_of(global_object, value)?);
    match allowed.iter().find(|candidate| **candidate == text) {
        Some(found) => Ok(Some(found)),
        None => Err(throw_coded_type_error(global_object, &format!("{name} must be one of {expectation}"), "ERR_INVALID_ARG_TYPE")),
    }
}

/// O estado de `new Request(input, init)`.
fn constructed_state(global_object: &JSGlobalObject, call: &HostCall) -> Result<RequestState, Thrown> {
    if call.argument_count() == 0 {
        return Err(construct_error(global_object, "1 argument required, but only 0 present."));
    }
    let input = call.argument(0);
    let init = call.argument(1);
    let base = state_of(input);
    let has_init = init.is_object();
    let option = |name: &[u8]| if has_init { property_of(global_object, init, name) } else { Ok(JSValue::undefined()) };
    // Ordem de leitura do `init` medida no bun: body, signal, headers, method, redirect, cache, mode; o `url` do `input`
    // (e o erro de URL inválida) vêm depois de todas.
    let body_init = option(b"body")?;
    let signal_init = option(b"signal")?;
    let headers_init = option(b"headers")?;
    let signal = if signal_init.is_undefined() {
        base.as_ref().map_or(JSValue::undefined(), |state| state.signal)
    } else if signal_init.is_null() {
        JSValue::undefined()
    } else if is_signal(signal_init) {
        signal_init
    } else {
        return Err(Thrown::type_error("Failed to construct 'Request': signal is not of type AbortSignal."));
    };
    // `headers` vazio (`{}`, `[]`, `new Headers()`) não substitui os cabeçalhos herdados.
    let headers = match (&base, headers_init.is_undefined()) {
        (Some(state), true) => clone_headers(global_object, state.headers),
        (None, true) => make_headers(global_object, JSValue::undefined())?,
        (_, false) => {
            let made = make_headers(global_object, headers_init)?;
            match &base {
                Some(state) if headers_is_empty(made) => clone_headers(global_object, state.headers),
                _ => made,
            }
        }
    };
    let mut method = base.as_ref().map_or_else(|| "GET".to_string(), |state| state.method.clone());
    let method_init = option(b"method")?;
    if !method_init.is_undefined_or_null() {
        method = normalized_method(&units_of(global_object, method_init)?);
    }
    let body = if body_init.is_undefined() {
        // Medido: `new Request(r)` com o corpo de `r` já lido não lança; o novo corpo existe mas está vazio (`text()` dá `""`).
        base.as_ref().and_then(|state| state.body.clone()).map(|body| {
            if body.is_used() {
                Body { bytes: Vec::new(), lazy_path: None, stream: None, from_stream: false, used: false, ..body }
            } else {
                Body { used: false, ..body }
            }
        })
    } else {
        let extracted = extract(global_object, body_init)?;
        if let Some(Extracted { header_type, .. }) = &extracted {
            if !header_type.is_empty() {
                append_default_header(headers, "content-type", header_type);
            }
        }
        extracted.map(Extracted::into_body)
    };
    let pick = |name: &str, allowed: &[&'static str], expectation: &str, inherited: Option<&'static str>, default: &'static str| {
        let chosen = if has_init { choice(global_object, init, name, allowed, expectation)? } else { None };
        Ok::<&'static str, Thrown>(chosen.or(inherited).unwrap_or(default))
    };
    let redirect = pick("redirect", REDIRECTS, "'follow', 'manual' or 'error'", base.as_ref().map(|state| state.redirect), "follow")?;
    let cache = pick("cache", CACHES, "'default', 'no-store', 'reload', 'no-cache', 'force-cache' or 'only-if-cached'", base.as_ref().map(|state| state.cache), "default")?;
    let mode = pick("mode", MODES, "'same-origin', 'no-cors', 'cors' or 'navigate'", base.as_ref().map(|state| state.mode), "cors")?;
    let url = match &base {
        Some(state) => state.url.clone(),
        None => canonical_url(global_object, &input_units(global_object, input)?)?,
    };
    Ok(RequestState { method, url, headers, body, redirect, cache, mode, signal })
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = constructed_state(global_object, call)?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    INSTANCES.with(|instances| instances.borrow_mut().insert(instance.encode(), state));
    Ok(instance)
}

/// Define um acessor que devolve um texto do estado.
macro_rules! text_getter {
    ($function:ident, $name:literal, $value:expr) => {
        fn $function(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            let value: fn(&RequestState) -> Vec<u16> = $value;
            Ok(string_value(global_object, &value(&accessor_state(call, $name)?)))
        }
    };
}

fn units_of_str(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

text_getter!(method_body, "method", |state| units_of_str(&state.method));
text_getter!(url_body, "url", |state| state.url.clone());
text_getter!(redirect_body, "redirect", |state| units_of_str(state.redirect));
text_getter!(cache_body, "cache", |state| units_of_str(state.cache));
text_getter!(mode_body, "mode", |state| units_of_str(state.mode));
text_getter!(credentials_body, "credentials", |_| units_of_str("include"));
text_getter!(destination_body, "destination", |_| Vec::new());
text_getter!(integrity_body, "integrity", |_| Vec::new());
text_getter!(referrer_body, "referrer", |_| Vec::new());
text_getter!(referrer_policy_body, "referrerPolicy", |_| Vec::new());

fn headers_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(accessor_state(call, "headers")?.headers)
}

/// `signal`: criado na primeira leitura quando o `init` não trouxe um; `new Request(r)` e `clone` herdam o que já existe.
fn signal_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = accessor_state(call, "signal")?;
    if !state.signal.is_undefined() {
        return Ok(state.signal);
    }
    let signal = new_signal(global_object)?;
    INSTANCES.with(|instances| {
        if let Some(stored) = instances.borrow_mut().get_mut(&call.this_value().encode()) {
            stored.signal = signal;
        }
    });
    Ok(signal)
}

fn body_used_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSValue::Bool(accessor_state(call, "bodyUsed")?.body.is_some_and(|body| body.is_used())))
}

/// `body`: sem corpo é `null`; com corpo, o `ReadableStream` (o mesmo objeto a cada leitura, guardado no estado).
fn body_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let mut state = accessor_state(call, "body")?;
    let stream = stream_property(global_object, &mut state.body);
    INSTANCES.with(|instances| {
        if let Some(stored) = instances.borrow_mut().get_mut(&call.this_value().encode()) {
            stored.body = state.body;
        }
    });
    Ok(stream)
}

/// As leituras: marcam o uso do corpo no estado guardado.
fn read_method(global_object: &JSGlobalObject, call: &HostCall, kind: ReadKind) -> HostResult {
    let state = method_state(global_object, call)?;
    let mut body = state.body.clone();
    let result = consume_kind(global_object, kind, state.headers, &mut body)?;
    INSTANCES.with(|instances| {
        if let Some(stored) = instances.borrow_mut().get_mut(&call.this_value().encode()) {
            stored.body = body;
        }
    });
    Ok(result)
}

fn text_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    read_method(global_object, call, ReadKind::Text)
}

fn json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    read_method(global_object, call, ReadKind::Json)
}

fn array_buffer_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    read_method(global_object, call, ReadKind::ArrayBuffer)
}

fn bytes_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    read_method(global_object, call, ReadKind::Bytes)
}

fn blob_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    read_method(global_object, call, ReadKind::Blob)
}

fn form_data_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    read_method(global_object, call, ReadKind::FormData)
}

/// Guarda o corpo de `this` de volta no estado (marcado como usado, ou com o stream dividido por `clone`).
fn store_body(this_value: JSValue, body: Option<Body>) {
    INSTANCES.with(|instances| {
        if let Some(stored) = instances.borrow_mut().get_mut(&this_value.encode()) {
            stored.body = body;
        }
    });
}

/// `textStream`: um `ReadableStream` de texto sobre o corpo (ver [`text_stream_of`]); marca o uso no estado guardado.
fn text_stream_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let mut state = method_state(global_object, call)?;
    let result = text_stream_of(global_object, &mut state.body)?;
    store_body(call.this_value(), state.body);
    Ok(result)
}

fn clone_method_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let mut state = method_state(global_object, call)?;
    let body = clone_body(global_object, &mut state.body)?;
    store_body(call.this_value(), state.body.take());
    let headers = clone_headers(global_object, state.headers);
    let prototype = REQUEST_PROTOTYPE.with(|slot| slot.borrow().expect("Request sem protótipo (instalação)"));
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype);
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    INSTANCES.with(|instances| instances.borrow_mut().insert(instance.encode(), RequestState { headers, body, ..state }));
    Ok(instance)
}

host_function!(call_fn, call_body);
host_function!(construct_fn, construct_body);
host_function!(method_fn, method_body);
host_function!(url_fn, url_body);
host_function!(redirect_fn, redirect_body);
host_function!(cache_fn, cache_body);
host_function!(mode_fn, mode_body);
host_function!(credentials_fn, credentials_body);
host_function!(destination_fn, destination_body);
host_function!(integrity_fn, integrity_body);
host_function!(referrer_fn, referrer_body);
host_function!(referrer_policy_fn, referrer_policy_body);
host_function!(headers_fn, headers_body);
host_function!(signal_fn, signal_body);
host_function!(body_used_fn, body_used_body);
host_function!(body_fn, body_body);
host_function!(text_fn, text_body);
host_function!(json_fn, json_body);
host_function!(array_buffer_fn, array_buffer_body);
host_function!(bytes_fn, bytes_body);
host_function!(blob_fn, blob_body);
host_function!(form_data_fn, form_data_body);
host_function!(text_stream_fn, text_stream_body);
host_function!(clone_fn, clone_method_body);

/// Instala `Request` no global.
pub fn install_request(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let define = |target: &crate::runtime::js_object::JSObject, name: &str, length: u32, function: NativeFunction| {
        put_direct_native_function_without_transition(
            vm,
            global_object,
            target,
            &Identifier::from_span(vm, name.as_bytes()),
            length,
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            DONT_DELETE,
        );
    };
    let parents = (global_object.object_prototype().as_value(), global_object.function_prototype().as_value());
    let (prototype, constructor) = create_native_subclass(global_object, parents, &PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "Request", 0, call_fn, construct_fn);
    let getter = |name: &str, function: NativeFunction| put_native_getter(vm, global_object, &prototype, name, function, Intrinsic::NoIntrinsic, DONT_DELETE);
    define(&prototype, "arrayBuffer", 0, array_buffer_fn);
    define(&prototype, "blob", 0, blob_fn);
    getter("body", body_fn);
    getter("bodyUsed", body_used_fn);
    define(&prototype, "bytes", 0, bytes_fn);
    getter("cache", cache_fn);
    define(&prototype, "clone", 1, clone_fn);
    getter("credentials", credentials_fn);
    getter("destination", destination_fn);
    define(&prototype, "formData", 0, form_data_fn);
    getter("headers", headers_fn);
    getter("integrity", integrity_fn);
    define(&prototype, "json", 0, json_fn);
    getter("method", method_fn);
    getter("mode", mode_fn);
    getter("redirect", redirect_fn);
    getter("referrer", referrer_fn);
    getter("referrerPolicy", referrer_policy_fn);
    getter("signal", signal_fn);
    define(&prototype, "text", 0, text_fn);
    define(&prototype, "textStream", 0, text_stream_fn);
    getter("url", url_fn);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    put_to_string_tag(vm, &prototype, "Request");
    REQUEST_PROTOTYPE.with(|slot| *slot.borrow_mut() = Some(prototype.as_value()));
    install_global_with_attributes(global_object, "Request", constructor.as_value(), 0);
}
