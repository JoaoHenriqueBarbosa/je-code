//! `Response` do global, só em memória (sem rede). O JavaScriptCore não o define: quem o instala é o bun (WebCore), como
//! propriedade de dados `writable`, `configurable` e ENUMERÁVEL (como `Headers`). Medido no bun 1.4.2
//! (`scripts/gen-fetch-types-golden.js`):
//!
//! - construtor nativo `length` 0, `name` "Response", estáticos `error`, `json` e `redirect` (todos `length` 0, enumeráveis,
//!   `writable`, NÃO configuráveis); protótipo, nesta ordem: `arrayBuffer`, `blob`, `body`, `bodyUsed`, `bytes`, `clone`(1),
//!   `formData`, `headers`, `json`, `ok`, `redirected`, `status`, `statusText`, `text`, `textStream`, `type`, `url`, e
//!   depois `constructor` (não enumerável) e `@@toStringTag`; métodos e acessores enumeráveis e NÃO configuráveis;
//! - sem `new`: `Response constructor cannot be invoked without 'new'` (`ERR_ILLEGAL_CONSTRUCTOR`); `this` alheio num
//!   método: `Expected this to be instanceof Response, but received ...` (`ERR_INVALID_THIS`); num acessor, sem código:
//!   `The Response.<nome> getter can only be used on instances of Response`;
//! - `new Response(body, init)`: `init` que não é objeto (nem `undefined`/`null`) lança `Failed to construct 'Response': The
//!   provided body value is not of type 'ResponseInit'` (`ERR_INVALID_ARG_TYPE`); `status` `undefined` vale 200, texto vira
//!   o prefixo inteiro (`'abc'` dá 0), número `NaN`/infinito vira `i64::MIN`, senão trunca; fora de 101 e 200..=599 lança
//!   `RangeError: The status provided (<n>) must be 101 or in the range of [200, 599]`; `statusText` `null`/`undefined`
//!   vale vazio, o resto passa por `ToString`; `headers` passa pelo `Headers` (um `Headers` existente é copiado);
//! - `Content-Type` implícito só de `Blob` com `type`, `URLSearchParams`, `FormData` e `Response.json`, e nunca sobrepõe o
//!   que `headers` já traz (ver `body.rs`);
//! - `Response.error()`: `type` "error", status 0; `Response.json(v, init)`: corpo `JSON.stringify(v)`
//!   (`Value is not JSON serializable` se der `undefined`), `application/json;charset=utf-8`; `Response.redirect(url, status)`:
//!   status só número em 301, 302, 303, 307, 308 (senão 302 se não for número, `RangeError` se for outro número),
//!   `Location` com o texto de `url` (vazio não põe cabeçalho);
//! - `status` 101 dá `type` "error" (medido no bun 1.4.2); 204, 205 e 304 com corpo não lançam;
//! - `clone` copia o corpo e os cabeçalhos; corpo usado lança `Body is disturbed or locked`.
//!
//! O `body` é o `ReadableStream` de `body.rs` (o do usuário, quando `new Response(stream)`); `textStream` devolve um `ReadableStream` de texto (`body::text_stream_of`).
//! `Response.error()` e `Response.redirect()` têm corpo vazio em stream (medido no bun 1.4.2): `body` é um `ReadableStream`
//! (o mesmo objeto a cada acesso, `locked` falso), `getReader().read()` dá `{done: true}`, `text()` dá `""` e marca
//! `bodyUsed`; já `new Response()` e `new Response(null)` têm `body` `null` e `bodyUsed` falso depois de `text()`.
//! `formData()` segue `body.rs`; o `Location` de `redirect` é a forma canônica do `URLParser` (o que não parseia vai
//! literal). `url` é vazio e `redirected` sempre falso, salvo nas respostas do `fetch` offline (`fetch.rs`), que trazem o
//! `url` do pedido.
//! O estado das instâncias fica em `thread_local` (zerado em `reset_for_program`), como `Blob`.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::body::{clone_body, consume_kind, extract, stream_property, text_stream_of, Body, Extracted, ReadKind};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::headers::{append_default_header, clone_headers, make_headers, remove_header};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::describe_received;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_value::{js_number, EncodedJSValue, JSValue};
use crate::runtime::json_object::{json_stringify, throw_json_error};
use crate::runtime::native_class_support::{create_native_subclass_with_statics, install_global_with_attributes, instance_structure};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::node_error::throw_coded_type_error;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM};
use crate::runtime::property_name::PropertyName;
use crate::runtime::web_iterable::{string_value, units_of};
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url::URL;

const JSON_TYPE: &[u8] = b"application/json;charset=utf-8";
const REDIRECT_STATUSES: [i64; 5] = [301, 302, 303, 307, 308];

#[derive(Clone)]
pub(crate) struct ResponseState {
    pub(crate) status: i64,
    pub(crate) status_text: Vec<u16>,
    /// O objeto `Headers` desta resposta (o mesmo a cada leitura de `headers`).
    pub(crate) headers: JSValue,
    pub(crate) body: Option<Body>,
    /// "default" ou "error".
    kind: &'static str,
    /// O `url` da resposta (vazio fora do `fetch`).
    pub(crate) url: Vec<u16>,
    /// Resposta do `fetch` ainda não lida: o `Content-Type` implícito sai dos cabeçalhos na primeira leitura (medido).
    fetched: bool,
    /// A resposta veio depois de seguir um redirecionamento do `fetch` de rede.
    redirected: bool,
}

static PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Response", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

thread_local! {
    /// As instâncias do programa (o valor codificado da célula) com o estado.
    static INSTANCES: RefCell<HashMap<EncodedJSValue, ResponseState>> = RefCell::new(HashMap::new());
    /// O protótipo de `Response`, guardado na instalação para as instâncias que os estáticos e `clone` criam.
    static RESPONSE_PROTOTYPE: RefCell<Option<JSValue>> = const { RefCell::new(None) };
}

/// Fim do programa (`cell_registry::reset_program_state`): o que foi guardado é do programa.
/// As raízes do coletor: o protótipo; o estado das respostas guarda valores e adia a coleta enquanto existir.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.value(RESPONSE_PROTOTYPE.with(|prototype| *prototype.borrow()));
}

/// O estado de cada `Response` é campo do objeto: `headers` e o stream do corpo vivem com ele.
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    crate::runtime::gc::side_table(&INSTANCES, marker, phase, |state, out| {
        out.push(state.headers);
        out.extend(state.body.as_ref().and_then(|body| body.stream));
    })
}

pub(crate) fn reset_for_program() {
    let _ = INSTANCES.try_with(|instances| instances.borrow_mut().clear());
}

pub(crate) fn state_of(value: JSValue) -> Option<ResponseState> {
    INSTANCES.with(|instances| instances.borrow().get(&value.encode()).cloned())
}

/// O estado de `this` para um método; senão o `TypeError` de `this` inválido.
fn method_state(global_object: &JSGlobalObject, call: &HostCall) -> Result<ResponseState, Thrown> {
    state_of(call.this_value()).ok_or_else(|| {
        let received = describe_received(global_object, call.this_value()).map(|text| format!(", but received {text}")).unwrap_or_default();
        throw_coded_type_error(global_object, &format!("Expected this to be instanceof Response{received}"), "ERR_INVALID_THIS")
    })
}

/// O estado de `this` para um acessor; senão o `TypeError` sem código.
fn accessor_state(call: &HostCall, name: &str) -> Result<ResponseState, Thrown> {
    state_of(call.this_value()).ok_or_else(|| Thrown::type_error(&format!("The Response.{name} getter can only be used on instances of Response")))
}

fn make_response(global_object: &JSGlobalObject, state: ResponseState) -> JSValue {
    let prototype = RESPONSE_PROTOTYPE.with(|slot| slot.borrow().expect("Response sem protótipo (instalação)"));
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype);
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    INSTANCES.with(|instances| instances.borrow_mut().insert(instance.encode(), state));
    instance
}

fn call_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Response constructor cannot be invoked without 'new'", "ERR_ILLEGAL_CONSTRUCTOR"))
}

pub(crate) fn property_of(global_object: &JSGlobalObject, object: JSValue, name: &[u8]) -> HostResult {
    get_value_property(global_object, object, &PropertyName::from_identifier(&Identifier::from_span(global_object.vm(), name)))
}

/// `status`, `statusText` e `headers` de um `ResponseInit`; `init` ausente vale tudo vazio.
fn init_parts(global_object: &JSGlobalObject, init: JSValue) -> Result<(JSValue, Vec<u16>, JSValue), Thrown> {
    if init.is_undefined_or_null() {
        return Ok((JSValue::undefined(), Vec::new(), make_headers(global_object, JSValue::undefined())?));
    }
    if !init.is_object() {
        return Err(throw_coded_type_error(
            global_object,
            "Failed to construct 'Response': The provided body value is not of type 'ResponseInit'",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    let headers_init = property_of(global_object, init, b"headers")?;
    let status = property_of(global_object, init, b"status")?;
    let text = property_of(global_object, init, b"statusText")?;
    let status_text = if text.is_undefined_or_null() { Vec::new() } else { units_of(global_object, text)? };
    Ok((status, status_text, make_headers(global_object, headers_init)?))
}

/// O `status` do construtor: `undefined` vale 200; o resto vira inteiro e tem de ser 101 ou 200..=599.
fn validated_status(global_object: &JSGlobalObject, raw: JSValue) -> Result<i64, Thrown> {
    if raw.is_undefined() {
        return Ok(200);
    }
    let number = pending_or(global_object, raw.to_number())?;
    let status = if raw.is_string() && number.is_nan() {
        0
    } else if !number.is_finite() {
        i64::MIN
    } else {
        number.trunc() as i64
    };
    if status == 101 || (200..=599).contains(&status) {
        Ok(status)
    } else {
        Err(Thrown::range_error(&format!("The status provided ({status}) must be 101 or in the range of [200, 599]")))
    }
}

fn build_state(extracted: Option<Extracted>, status: i64, status_text: Vec<u16>, headers: JSValue, kind: &'static str) -> ResponseState {
    if let Some(extracted) = &extracted {
        if !extracted.header_type.is_empty() {
            append_default_header(headers, "content-type", &extracted.header_type);
        }
    }
    ResponseState { status, status_text, headers, body: extracted.map(Extracted::into_body), kind, url: Vec::new(), fetched: false, redirected: false }
}

/// A `Response` que o `fetch` devolve: status 200, `Content-Type` implícito (se `content_type` não é vazio), `url` dado.
pub(crate) fn make_fetched_response(global_object: &JSGlobalObject, status_text: &str, url: Vec<u16>, content_type: &[u8], body: Body) -> HostResult {
    let headers = make_headers(global_object, JSValue::undefined())?;
    if !content_type.is_empty() {
        append_default_header(headers, "content-type", content_type);
    }
    let state = ResponseState { status: 200, status_text: status_text.encode_utf16().collect(), headers, body: Some(body), kind: "default", url, fetched: true, redirected: false };
    Ok(make_response(global_object, state))
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let extracted = extract(global_object, call.argument(0))?;
    let (status_raw, status_text, headers) = init_parts(global_object, call.argument(1))?;
    let status = validated_status(global_object, status_raw)?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    INSTANCES.with(|instances| instances.borrow_mut().insert(instance.encode(), build_state(extracted, status, status_text, headers, if status == 101 { "error" } else { "default" })));
    Ok(instance)
}

/// `Response.error()`.
fn error_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let headers = make_headers(global_object, JSValue::undefined())?;
    Ok(make_response(global_object, build_state(Some(Extracted::default()), 0, Vec::new(), headers, "error")))
}

/// `Response.json(value, init)`: sem argumentos o corpo é vazio.
fn json_static_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let bytes = if call.argument_count() == 0 {
        Vec::new()
    } else if call.argument(0).is_big_int() {
        // Medido no bun 1.4.2: só o BigInt de topo tem esta mensagem; dentro de objeto é a do `JSON.stringify`.
        return Err(Thrown::type_error("Do not know how to serialize a BigInt"));
    } else {
        match json_stringify(global_object, call.argument(0), JSValue::undefined(), JSValue::undefined()) {
            Ok(Some(text)) => {
                let units: Vec<u16> = (0..text.length()).map(|index| text.code_unit_at(index)).collect();
                String::from_utf16_lossy(&units).into_bytes()
            }
            Ok(None) => return Err(Thrown::type_error("Value is not JSON serializable")),
            Err(error) => {
                throw_json_error(global_object, error);
                return Err(Thrown::Pending);
            }
        }
    };
    let (status_raw, status_text, headers) = init_parts(global_object, call.argument(1))?;
    let status = validated_status(global_object, status_raw)?;
    let extracted = Extracted { bytes, header_type: JSON_TYPE.to_vec(), blob_type: JSON_TYPE.to_vec(), ..Extracted::default() };
    Ok(make_response(global_object, build_state(Some(extracted), status, status_text, headers, "default")))
}

/// `Response.redirect(url, status)`: o segundo argumento é o status (número) ou um `ResponseInit`.
fn redirect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let units = units_of(global_object, call.argument(0))?;
    let parsed = URL::from_string(&WtfString::from_utf16(&units));
    // O `Location` é a forma canônica do `URLParser`; o que não parseia vai literal (medido).
    let location = if parsed.is_valid() { parsed.string().utf8(ConversionMode::LenientConversion) } else { String::from_utf16_lossy(&units).into_bytes() };
    let second = call.argument(1);
    let (status_raw, status_text, headers) = if second.is_object() { init_parts(global_object, second)? } else { (second, Vec::new(), make_headers(global_object, JSValue::undefined())?) };
    let status = if status_raw.is_number() { status_raw.as_number().trunc() as i64 } else { 302 };
    if !REDIRECT_STATUSES.contains(&status) {
        return Err(Thrown::range_error("Failed to execute 'redirect' on 'Response': Invalid status code"));
    }
    if !units.is_empty() {
        append_default_header(headers, "location", &location);
    }
    Ok(make_response(global_object, build_state(Some(Extracted::default()), status, status_text, headers, "default")))
}

fn status_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_number(accessor_state(call, "status")?.status as f64))
}

fn status_text_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(string_value(global_object, &accessor_state(call, "statusText")?.status_text))
}

fn ok_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSValue::Bool((200..=299).contains(&accessor_state(call, "ok")?.status)))
}

fn headers_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(accessor_state(call, "headers")?.headers)
}

fn body_used_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSValue::Bool(accessor_state(call, "bodyUsed")?.body.is_some_and(|body| body.is_used())))
}

fn url_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(string_value(global_object, &accessor_state(call, "url")?.url))
}

fn redirected_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSValue::Bool(accessor_state(call, "redirected")?.redirected))
}

fn type_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let kind = accessor_state(call, "type")?.kind;
    Ok(string_value(global_object, &kind.encode_utf16().collect::<Vec<u16>>()))
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
    read_response(global_object, call.this_value(), method_state(global_object, call)?, kind)
}

/// A leitura `kind` do corpo de `response` (o `Response` cujo estado é `state`): a promessa do método de mesmo nome.
pub(crate) fn read_response(global_object: &JSGlobalObject, response: JSValue, state: ResponseState, kind: ReadKind) -> HostResult {
    let mut body = state.body.clone();
    let result = consume_kind(global_object, kind, state.headers, &mut body)?;
    if state.fetched {
        remove_header(state.headers, "content-type");
    }
    INSTANCES.with(|instances| {
        if let Some(stored) = instances.borrow_mut().get_mut(&response.encode()) {
            stored.body = body;
            stored.fetched = false;
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
    Ok(make_response(global_object, ResponseState { headers, body, ..state }))
}

host_function!(call_fn, call_body);
host_function!(construct_fn, construct_body);
host_function!(error_fn, error_body);
host_function!(json_static_fn, json_static_body);
host_function!(redirect_fn, redirect_body);
host_function!(status_fn, status_body);
host_function!(status_text_fn, status_text_body);
host_function!(ok_fn, ok_body);
host_function!(headers_fn, headers_body);
host_function!(body_used_fn, body_used_body);
host_function!(url_fn, url_body);
host_function!(redirected_fn, redirected_body);
host_function!(type_fn, type_body);
host_function!(body_fn, body_body);
host_function!(text_fn, text_body);
host_function!(json_fn, json_body);
host_function!(array_buffer_fn, array_buffer_body);
host_function!(bytes_fn, bytes_body);
host_function!(blob_fn, blob_body);
host_function!(form_data_fn, form_data_body);
host_function!(text_stream_fn, text_stream_body);
host_function!(clone_fn, clone_method_body);

/// Instala `Response` no global.
pub fn install_response(global_object: &JSGlobalObject) {
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
    // Os estáticos entram entre `name` e `prototype` (ordem de `Object.getOwnPropertyNames(Response)` no bun).
    let parents = (global_object.object_prototype().as_value(), global_object.function_prototype().as_value());
    let (prototype, constructor) = create_native_subclass_with_statics(global_object, parents, &PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "Response", 0, call_fn, construct_fn, |constructor| {
        define(constructor, "error", 0, error_fn);
        define(constructor, "json", 0, json_static_fn);
        define(constructor, "redirect", 0, redirect_fn);
    });    let getter = |name: &str, function: NativeFunction| put_native_getter(vm, global_object, &prototype, name, function, Intrinsic::NoIntrinsic, DONT_DELETE);
    define(&prototype, "arrayBuffer", 0, array_buffer_fn);
    define(&prototype, "blob", 0, blob_fn);
    getter("body", body_fn);
    getter("bodyUsed", body_used_fn);
    define(&prototype, "bytes", 0, bytes_fn);
    define(&prototype, "clone", 1, clone_fn);
    define(&prototype, "formData", 0, form_data_fn);
    getter("headers", headers_fn);
    define(&prototype, "json", 0, json_fn);
    getter("ok", ok_fn);
    getter("redirected", redirected_fn);
    getter("status", status_fn);
    getter("statusText", status_text_fn);
    define(&prototype, "text", 0, text_fn);
    define(&prototype, "textStream", 0, text_stream_fn);
    getter("type", type_fn);
    getter("url", url_fn);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    put_to_string_tag(vm, &prototype, "Response");
    RESPONSE_PROTOTYPE.with(|slot| *slot.borrow_mut() = Some(prototype.as_value()));
    install_global_with_attributes(global_object, "Response", constructor.as_value(), 0);
}

/// `markFetched(resposta, url, redirecionada)` para o `bun_http.js`: a `Response` que o `fetch` de rede devolve traz o
/// `url` final e o `redirected` (o construtor de `Response` deixa o `url` vazio).
fn mark_fetched_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let response = call.argument(0);
    let url = units_of(_global_object, call.argument(1))?;
    let redirected = call.argument(2).to_boolean();
    INSTANCES.with(|instances| {
        if let Some(stored) = instances.borrow_mut().get_mut(&response.encode()) {
            stored.url = url;
            stored.redirected = redirected;
        }
    });
    Ok(JSValue::undefined())
}
host_function!(pub mark_fetched, mark_fetched_body);

/// `responseWire(resposta)` para o `bun_http.js`: o corpo que vai para o fio. `undefined` sem corpo; senão
/// `[bytes ou stream, tipo]` (o `ReadableStream` do usuário, ou os bytes e o tipo que a leitura herda). Marca o corpo
/// como lido.
fn response_wire_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let response = call.argument(0);
    let Some(mut state) = state_of(response) else { return Ok(JSValue::undefined()) };
    let Some(body) = state.body.as_mut() else { return Ok(JSValue::undefined()) };
    if body.is_used() {
        return Err(throw_coded_type_error(global_object, "Body already used", "ERR_BODY_ALREADY_USED"));
    }
    body.used = true;
    let mut payload_is_empty = false;
    let payload = match (body.from_stream, body.stream) {
        (true, Some(stream)) => {
            payload_is_empty = true;
            stream
        }
        _ => {
            let loaded;
            let content: &[u8] = match &body.lazy_path {
                Some(path) => {
                    loaded = crate::runtime::fetch::read_file_body(global_object, path).unwrap_or_default();
                    &loaded
                }
                None => &body.bytes,
            };
            payload_is_empty = content.is_empty();
            let array = crate::runtime::uint8_array_base64::create_uint8_array(global_object, content.len())?;
            array.with_vector_mut(|destination| destination[..content.len()].copy_from_slice(content));
            array.as_value()
        }
    };
    // O tipo que o servidor manda sem `Content-Type` (medido no bun 1.4.2): o do corpo; sem ele, `text/plain` para um
    // `Blob` e `application/octet-stream` para bytes; corpo vazio não manda tipo nenhum.
    let wire_type = match (body.blob_type.is_empty(), body.from_blob, payload_is_empty) {
        (_, _, true) => String::new(),
        (false, _, false) => String::from_utf8_lossy(&body.blob_type).into_owned(),
        (true, true, false) => "text/plain;charset=utf-8".to_owned(),
        (true, false, false) => "application/octet-stream".to_owned(),
    };
    let kind = crate::runtime::process_shape::text_value(global_object.vm(), &wire_type);
    INSTANCES.with(|instances| {
        if let Some(stored) = instances.borrow_mut().get_mut(&response.encode()) {
            stored.body = state.body.clone();
        }
    });
    Ok(crate::runtime::process_shape::array_value(global_object, &[payload, kind]))
}
host_function!(pub response_wire, response_wire_body);
