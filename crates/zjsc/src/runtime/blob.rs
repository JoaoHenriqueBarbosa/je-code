//! `Blob` do global, só em memória (sem disco nem rede). O JavaScriptCore não o define: quem o instala é o bun
//! (WebCore), como propriedade de dados `writable`, `configurable` e NÃO enumerável. Medido no bun 1.4.2
//! (`scripts/gen-blob-golden.js`):
//!
//! - construtor nativo `length` 0, `name` "Blob", chaves próprias `length`, `name`, `prototype`;
//! - protótipo, nesta ordem: `arrayBuffer`, `bytes`, `delete`, `exists`, `formData`, `image`, `json`, `lastModified`,
//!   `name`, `size`, `slice`(2), `stat`, `stream`(1), `text`, `type`, `unlink`, `write`(2), `writer`(1), `constructor`
//!   (não enumerável) e `@@toStringTag`; métodos e acessores enumeráveis e NÃO configuráveis;
//! - sem `new`: `Blob constructor cannot be invoked without 'new'` (`ERR_ILLEGAL_CONSTRUCTOR`); `this` alheio num método:
//!   `Expected this to be instanceof Blob, but received ...` (`ERR_INVALID_THIS`); num acessor, sem código:
//!   `The Blob.<nome> getter can only be used on instances of Blob`;
//! - partes: `undefined`/`null` são vazias, typed array/`ArrayBuffer`/`DataView`/`Blob` soltos valem como suas partes, um
//!   array leva cada item (os mesmos tipos viram bytes, o resto passa por `ToString`, string em UTF-8 com U+FFFD), o
//!   resto lança `new Blob() expects an Array` (`ERR_INVALID_ARG_TYPE`); `endings` é ignorado (Linux);
//! - `type`: só string vale, em minúsculas, e qualquer byte fora de 0x20..0x7e zera;
//! - `slice`: argumento string vira o `contentType`, índices só se forem número (truncados, negativos contam do fim), o
//!   resultado não herda o `type` mas herda o `name`; `lastModified` de um Blob em memória é 2^52-1;
//! - `text`, `arrayBuffer`, `bytes` e `json` devolvem promessas já resolvidas (`json` rejeita com o `SyntaxError` do JSON).
//!
//! Arquivo, num Blob em memória (medido): `delete`/`unlink`/`write`/`writer` lançam `Cannot write to a Blob backed by
//! bytes, which are always read-only` (`ERR_INVALID_ARG_TYPE`), ou `Cannot write to a detached Blob` (`Error` sem código)
//! se vazio; `writer` com argumento não objeto lança `options must be an object or undefined`; `exists` resolve `true`;
//! `stat` devolve `undefined` síncrono; `formData` rejeita `Error: Invalid encoding` fora de vazio/urlencoded/multipart;
//! `image` vazio lança `Image() input must be ...`.
//! `stream` devolve um `ReadableStream` novo a cada chamada (`streams/body_stream.rs`). Pendente (ver `unsupported_body`):
//! `image` com conteúdo (devolve `Image`), que depende de classe ainda ausente. `formData` parseia (`form_data_parse.rs`).
//! O estado das instâncias fica em `thread_local` (zerado em `reset_for_program`), como `URLSearchParams`.

use crate::runtime::js_promise_host::PromiseHost;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::body::{read_body, Read};
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_array::JSArray;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::describe_received;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_promise::JSPromise;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_number, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{create_native_class_with_length, install_global_with_attributes, instance_structure, put_native_accessor, throw_coded_type_error};
use crate::runtime::node_error::throw_plain_error;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM};
use crate::runtime::property_name::PropertyName;
use crate::runtime::text_decoder::input_bytes;
use crate::wtf::text::wtf_string::String as WtfString;

/// `lastModified` de um Blob que não é arquivo: `2^52 - 1`.
pub(crate) const MEMORY_LAST_MODIFIED: f64 = 4503599627370495.0;

#[derive(Clone, Default)]
pub(crate) struct BlobState {
    pub(crate) bytes: Vec<u8>,
    pub(crate) content_type: Vec<u8>,
    pub(crate) name: Option<Vec<u16>>,
    /// `true` num `File` (e no que o `FormData` devolve): `instanceof File` vale.
    pub(crate) is_file: bool,
    /// O `lastModified` de um arquivo; `None` num Blob em memória (`2^52 - 1`).
    pub(crate) last_modified: Option<f64>,
}

static PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Blob", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

thread_local! {
    /// As instâncias do programa (o valor codificado da célula) com o conteúdo.
    static INSTANCES: RefCell<HashMap<EncodedJSValue, BlobState>> = RefCell::new(HashMap::new());
    /// O protótipo de `Blob`, guardado na instalação para as instâncias que `slice` cria.
    static BLOB_PROTOTYPE: RefCell<Option<JSValue>> = const { RefCell::new(None) };
}

/// O protótipo de `Blob` (e de `File`), guardado na instalação.
pub(crate) fn blob_prototype() -> JSValue {
    BLOB_PROTOTYPE.with(|slot| slot.borrow().expect("Blob sem protótipo (instalação)"))
}

/// Fim do programa (`cell_registry::reset_program_state`): o que foi guardado é do programa.
/// As raízes do coletor: o protótipo e as instâncias (o estado é só de bytes).
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.value(BLOB_PROTOTYPE.with(|prototype| *prototype.borrow()));
    INSTANCES.with(|instances| instances.borrow().keys().for_each(|instance| marker.encoded(*instance)));
}

pub(crate) fn reset_for_program() {
    let _ = INSTANCES.try_with(|instances| instances.borrow_mut().clear());
    crate::runtime::blob_file::reset_for_program();
}

pub(crate) fn state_of(value: JSValue) -> Option<BlobState> {
    INSTANCES.with(|instances| instances.borrow().get(&value.encode()).cloned())
}

/// Registra a instância recém-criada (`new File`) com o estado inicial.
pub(crate) fn register_instance(instance: JSValue, state: BlobState) {
    INSTANCES.with(|instances| instances.borrow_mut().insert(instance.encode(), state));
}

/// O estado de `this` para um método; senão o `TypeError` de `this` inválido.
fn method_state(global_object: &JSGlobalObject, call: &HostCall) -> Result<BlobState, Thrown> {
    state_of(call.this_value()).ok_or_else(|| {
        let received = describe_received(global_object, call.this_value()).map(|text| format!(", but received {text}")).unwrap_or_default();
        throw_coded_type_error(global_object, &format!("Expected this to be instanceof Blob{received}"), "ERR_INVALID_THIS")
    })
}

/// O estado de `this` para um acessor; senão o `TypeError` sem código.
fn accessor_state(call: &HostCall, name: &str) -> Result<BlobState, Thrown> {
    state_of(call.this_value()).ok_or_else(|| Thrown::type_error(&format!("The Blob.{name} getter can only be used on instances of Blob")))
}

fn latin1_value(global_object: &JSGlobalObject, bytes: &[u8]) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(bytes)))
}

/// Os tipos que o bun troca pela constante canônica do `MimeType` (medido no bun 1.4.2 contra todos os tipos da tabela
/// MIME): a busca é no texto como veio, então `Text/plain` só vira `text/plain`.
const CANONICAL_TYPES: &[(&[u8], &[u8])] = &[
    (b"text/plain", b"text/plain;charset=utf-8"),
    (b"text/html", b"text/html;charset=utf-8"),
    (b"application/json", b"application/json;charset=utf-8"),
    (b"text/javascript", b"text/javascript;charset=utf-8"),
    (b"application/javascript", b"text/javascript;charset=utf-8"),
    (b"text/css", b"text/css;charset=utf-8"),
    (b"text/jsx", b"text/javascript;charset=utf-8"),
    (b"application/x-www-form-urlencoded", b"application/x-www-form-urlencoded;charset=UTF-8"),
];

/// O `type` aceito: só ASCII imprimível (0x20..0x7e), em minúsculas, ou a constante canônica; senão vazio.
pub(crate) fn normalized_type(value: JSValue) -> Vec<u8> {
    if !value.is_string() {
        return Vec::new();
    }
    let text = value.to_wtf_string();
    let mut out = Vec::with_capacity(text.length() as usize);
    let mut lowered = false;
    for index in 0..text.length() {
        match u8::try_from(text.code_unit_at(index)) {
            Ok(byte) if (0x20..=0x7e).contains(&byte) => {
                lowered |= byte.is_ascii_uppercase();
                out.push(byte.to_ascii_lowercase());
            }
            _ => return Vec::new(),
        }
    }
    match CANONICAL_TYPES.iter().find(|(key, _)| !lowered && *key == out.as_slice()) {
        Some((_, canonical)) => canonical.to_vec(),
        None => out,
    }
}

fn call_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Blob constructor cannot be invoked without 'new'", "ERR_ILLEGAL_CONSTRUCTOR"))
}

/// Os bytes de uma parte: Blob, typed array, `DataView` ou `ArrayBuffer` direto; `None` para o resto.
pub(crate) fn binary_bytes(value: JSValue) -> Option<Vec<u8>> {
    state_of(value).map(|state| state.bytes).or_else(|| input_bytes(value))
}

pub(crate) fn string_bytes(global_object: &JSGlobalObject, value: JSValue) -> Result<Vec<u8>, Thrown> {
    let text = pending_or(global_object, value.to_wtf_string())?;
    let units: Vec<u16> = (0..text.length()).map(|index| text.code_unit_at(index)).collect();
    Ok(String::from_utf16_lossy(&units).into_bytes())
}

/// Os bytes de `new Blob(parts)`.
pub(crate) fn parts_bytes(global_object: &JSGlobalObject, parts: JSValue) -> Result<Vec<u8>, Thrown> {
    if parts.is_undefined_or_null() {
        return Ok(Vec::new());
    }
    // Direto, só ArrayBuffer e visões (medido no bun 1.4.2): um `Blob` ou `File` como lista de partes lança.
    if state_of(parts).is_none() {
        if let Some(bytes) = input_bytes(parts) {
            return Ok(bytes);
        }
    }
    let Some(array) = JSArray::from_value(&parts) else {
        return Err(throw_coded_type_error(global_object, "new Blob() expects an Array", "ERR_INVALID_ARG_TYPE"));
    };
    let mut out = Vec::new();
    let mut index = 0;
    while index < array.length() {
        // Buraco do array esparso não é parte (medido: `[, 'a']` tem 1 byte).
        if !array.object().has_property_by_index(global_object.vm(), index) {
            index += 1;
            continue;
        }
        let item = array.object().get_by_index(global_object.vm(), index);
        // `null` e `undefined` na lista não são parte (medido no bun 1.4.2: contribuem com 0 bytes).
        if item.is_undefined_or_null() {
            index += 1;
            continue;
        }
        match binary_bytes(item) {
            Some(bytes) => out.extend_from_slice(&bytes),
            None => out.extend_from_slice(&string_bytes(global_object, item)?),
        }
        index += 1;
    }
    Ok(out)
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let bytes = parts_bytes(global_object, call.argument(0))?;
    let options = call.argument(1);
    let content_type = if options.is_object() {
        normalized_type(get_value_property(global_object, options, &PropertyName::from_identifier(&Identifier::from_span(global_object.vm(), b"type")))?)
    } else {
        Vec::new()
    };
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    INSTANCES.with(|instances| instances.borrow_mut().insert(instance.encode(), BlobState { bytes, content_type, ..BlobState::default() }));
    Ok(instance)
}

pub(crate) fn make_blob(global_object: &JSGlobalObject, state: BlobState) -> JSValue {
    let prototype = BLOB_PROTOTYPE.with(|slot| slot.borrow().expect("Blob sem protótipo (instalação)"));
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype);
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    INSTANCES.with(|instances| instances.borrow_mut().insert(instance.encode(), state));
    instance
}

fn size_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = accessor_state(call, "size")?;
    Ok(js_number(crate::runtime::blob_file::size(call.this_value()).unwrap_or(state.bytes.len() as f64)))
}

fn type_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(latin1_value(global_object, &accessor_state(call, "type")?.content_type))
}

fn last_modified_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = accessor_state(call, "lastModified")?;
    Ok(js_number(crate::runtime::blob_file::last_modified(call.this_value()).unwrap_or_else(|| state.last_modified.unwrap_or(MEMORY_LAST_MODIFIED))))
}

/// Sem nome: `undefined` (também num `File` que o `FormData` devolveu de um Blob sem nome, medido).
fn name_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = accessor_state(call, "name")?;
    Ok(state.name.map_or_else(JSValue::undefined, |units| JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf16(&units)))))
}

/// `name = valor`: string define, `null`/`undefined` limpam, o resto é ignorado.
fn set_name_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    accessor_state(call, "name")?;
    let value = call.argument(0);
    let name = if value.is_string() {
        let text = value.to_wtf_string();
        Some((0..text.length()).map(|index| text.code_unit_at(index)).collect::<Vec<u16>>())
    } else if value.is_undefined_or_null() {
        None
    } else {
        return Ok(JSValue::undefined());
    };
    INSTANCES.with(|instances| instances.borrow_mut().get_mut(&call.this_value().encode()).map(|state| state.name = name));
    Ok(JSValue::undefined())
}

/// Índice de `slice`: só número conta (truncado, saturando); negativo vale a partir do fim.
fn slice_index(argument: JSValue, size: i64, default: i64) -> i64 {
    if !argument.is_number() {
        return default;
    }
    let value = argument.as_number() as i64;
    if value < 0 {
        value.wrapping_add(size).max(0)
    } else {
        value.min(size)
    }
}

fn slice_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = method_state(global_object, call)?;
    let file_size = crate::runtime::blob_file::size(call.this_value());
    if state.bytes.is_empty() && file_size.is_none() {
        return Ok(make_blob(global_object, BlobState::default()));
    }
    let size = file_size.map_or(state.bytes.len() as i64, |size| size as i64);
    let mut arguments = [call.argument(0), call.argument(1), call.argument(2)];
    if arguments[0].is_string() {
        arguments = [JSValue::undefined(), JSValue::undefined(), arguments[0]];
    } else if arguments[1].is_string() {
        arguments = [arguments[0], JSValue::undefined(), arguments[1]];
    }
    let start = slice_index(arguments[0], size, 0);
    let end = slice_index(arguments[1], size, size);
    let content_type = normalized_type(arguments[2]);
    if let Some(sliced) = crate::runtime::blob_file::slice(global_object, call.this_value(), start, end, content_type.clone()) {
        return Ok(sliced);
    }
    let bytes = if start < end { state.bytes[start as usize..end as usize].to_vec() } else { Vec::new() };
    Ok(make_blob(global_object, BlobState { bytes, content_type, name: state.name, is_file: state.is_file, last_modified: state.last_modified }))
}

pub(crate) fn resolved_promise(global_object: &JSGlobalObject, value: JSValue) -> JSValue {
    let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
    promise.resolve(global_object, value);
    promise.as_value()
}

/// O estado de `this` com o conteúdo, passado a `read`; num Blob de arquivo o arquivo é lido na hora e uma falha de
/// leitura rejeita a promessa.
fn with_content(global_object: &JSGlobalObject, call: &HostCall, read: impl FnOnce(&BlobState) -> HostResult) -> HostResult {
    let mut state = method_state(global_object, call)?;
    if let Some(backing) = crate::runtime::blob_file::backing_of(call.this_value()) {
        match crate::runtime::blob_file::read(&backing) {
            Ok(bytes) => state.bytes = bytes,
            Err(error) => return Ok(crate::runtime::blob_file::rejected(global_object, &error)),
        }
    }
    read(&state)
}

fn text_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_content(global_object, call, |state| {
        // `Blob.text()` do bun decodifica como UTF-16LE quando o conteúdo começa com a BOM `FF FE` (medido no bun 1.4.2:
        // `[ff fe 41]` dá `""`, o byte ímpar final cai). `Response.text()` não faz isso.
        if let Some(rest) = state.bytes.strip_prefix(&[0xff, 0xfe]) {
            let units: Vec<u16> = rest.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
            let text = WtfString::from_utf8(String::from_utf16_lossy(&units).as_bytes());
            return Ok(resolved_promise(global_object, JSValue::from_js_string(js_string(global_object.vm(), &text))));
        }
        read_body(global_object, Read::Text, &state.bytes)
    })
}

fn array_buffer_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_content(global_object, call, |state| read_body(global_object, Read::ArrayBuffer, &state.bytes))
}

fn bytes_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_content(global_object, call, |state| read_body(global_object, Read::Bytes, &state.bytes))
}

fn json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_content(global_object, call, |state| read_body(global_object, Read::Json, &state.bytes))
}

/// `delete`, `unlink`, `write` e `writer` num Blob em memória: o vazio não tem armazenamento ("detached"), o resto é
/// somente leitura. `writer` valida antes o argumento (`options must be an object or undefined`).
fn write_refused(global_object: &JSGlobalObject, state: &BlobState) -> Thrown {
    if state.bytes.is_empty() {
        throw_plain_error(global_object, "Cannot write to a detached Blob")
    } else {
        throw_coded_type_error(global_object, "Cannot write to a Blob backed by bytes, which are always read-only", "ERR_INVALID_ARG_TYPE")
    }
}

/// `delete` e `unlink`: num Blob de arquivo removem o arquivo.
fn delete_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = method_state(global_object, call)?;
    crate::runtime::blob_file::delete(global_object, call.this_value()).ok_or_else(|| write_refused(global_object, &state))
}

/// `write(dado, opções)`: num Blob de arquivo é o `Bun.write` com ele de destino.
fn write_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = method_state(global_object, call)?;
    if crate::runtime::blob_file::backing_of(call.this_value()).is_none() {
        return Err(write_refused(global_object, &state));
    }
    crate::runtime::blob_file::call_module(global_object, "write", &[call.this_value(), call.argument(0), call.argument(1)])
}

fn writer_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = method_state(global_object, call)?;
    let options = call.argument(0);
    if !options.is_undefined() && !options.is_object() {
        return Err(throw_coded_type_error(global_object, "options must be an object or undefined", "ERR_INVALID_ARG_TYPE"));
    }
    if crate::runtime::blob_file::backing_of(call.this_value()).is_some() {
        return crate::runtime::blob_file::call_module(global_object, "writer", &[call.this_value()]);
    }
    Err(write_refused(global_object, &state))
}

/// Um Blob em memória sempre "existe".
fn exists_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    method_state(global_object, call)?;
    Ok(crate::runtime::blob_file::exists(global_object, call.this_value()).unwrap_or_else(|| resolved_promise(global_object, JSValue::Bool(true))))
}

/// `stat` de um Blob que não é arquivo devolve `undefined`, síncrono.
fn stat_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    method_state(global_object, call)?;
    crate::runtime::blob_file::stat(global_object, call.this_value()).unwrap_or_else(|| Ok(JSValue::undefined()))
}

/// `formData`: corpo vazio vira `FormData` vazio; o resto segue `form_data_parse::parse_body` (urlencoded e multipart),
/// e o erro (`Invalid encoding`, `FormData encoding failed: ...`) rejeita a promessa.
fn form_data_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_content(global_object, call, |state| match crate::runtime::form_data_parse::parse_body(&state.bytes, &state.content_type) {
        Ok(entries) => Ok(resolved_promise(global_object, crate::runtime::form_data::make_form_data(global_object, entries))),
        Err(message) => {
            let error = crate::runtime::error::create_error(global_object, &WtfString::from_utf8(message.as_bytes())).as_value();
            Ok(JSPromise::rejected_promise(global_object, error).as_value())
        }
    })
}

/// `image` de um Blob vazio lança; com conteúdo devolve um `Image` (ainda ausente).
fn image_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = method_state(global_object, call)?;
    if state.bytes.is_empty() {
        return Err(throw_coded_type_error(
            global_object,
            "Image() input must be a path string, data: URL, ArrayBuffer, TypedArray or Blob",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    unsupported_body(global_object, call)
}

/// `stream`: um `ReadableStream` novo a cada chamada, sobre uma cópia dos bytes (ver `streams/body_stream.rs`).
fn stream_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = method_state(global_object, call)?;
    if let Some(backing) = crate::runtime::blob_file::backing_of(call.this_value()) {
        // Num arquivo a falha aparece na leitura do stream, não na chamada de `stream()` (medido no bun 1.4.2).
        return Ok(match crate::runtime::blob_file::read(&backing) {
            Ok(bytes) => crate::runtime::streams::bytes_stream(global_object, &bytes),
            Err(error) => crate::runtime::streams::errored_stream(global_object, crate::runtime::blob_file::error_value(global_object, &error)),
        });
    }
    Ok(crate::runtime::streams::bytes_stream(global_object, &state.bytes))
}

/// O que sobra de `image` com conteúdo (classe `Image` ainda não portada): só a forma existe.
fn unsupported_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    method_state(global_object, call)?;
    Err(throw_coded_type_error(global_object, "Not implemented", "ERR_METHOD_NOT_IMPLEMENTED"))
}

host_function!(call_fn, call_body);
host_function!(construct_fn, construct_body);
host_function!(size_fn, size_body);
host_function!(type_fn, type_body);
host_function!(last_modified_fn, last_modified_body);
host_function!(name_fn, name_body);
host_function!(set_name_fn, set_name_body);
host_function!(slice_fn, slice_body);
host_function!(text_fn, text_body);
host_function!(array_buffer_fn, array_buffer_body);
host_function!(bytes_fn, bytes_body);
host_function!(json_fn, json_body);
host_function!(unsupported_fn, unsupported_body);
host_function!(stream_fn, stream_body);
host_function!(delete_fn, delete_body);
host_function!(write_fn, write_body);
host_function!(writer_fn, writer_body);
host_function!(exists_fn, exists_body);
host_function!(stat_fn, stat_body);
host_function!(form_data_fn, form_data_body);
host_function!(image_fn, image_body);

/// Instala `Blob` no global.
pub fn install_blob(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let (prototype, constructor) = create_native_class_with_length(global_object, &PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "Blob", 0, call_fn, construct_fn);
    let method = |name: &str, length: u32, function: NativeFunction| {
        put_direct_native_function_without_transition(
            vm,
            global_object,
            &prototype,
            &Identifier::from_span(vm, name.as_bytes()),
            length,
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            DONT_DELETE,
        );
    };
    method("arrayBuffer", 0, array_buffer_fn);
    method("bytes", 0, bytes_fn);
    method("delete", 0, delete_fn);
    method("exists", 0, exists_fn);
    method("formData", 0, form_data_fn);
    method("image", 0, image_fn);
    method("json", 0, json_fn);
    put_native_accessor(vm, global_object, &prototype, "lastModified", last_modified_fn, None, DONT_DELETE);
    put_native_accessor(vm, global_object, &prototype, "name", name_fn, Some(set_name_fn), DONT_DELETE);
    put_native_getter(vm, global_object, &prototype, "size", size_fn, Intrinsic::NoIntrinsic, DONT_DELETE);
    method("slice", 2, slice_fn);
    method("stat", 0, stat_fn);
    method("stream", 1, stream_fn);
    method("text", 0, text_fn);
    put_native_accessor(vm, global_object, &prototype, "type", type_fn, None, DONT_DELETE);
    method("unlink", 0, delete_fn);
    method("write", 2, write_fn);
    method("writer", 1, writer_fn);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    put_to_string_tag(vm, &prototype, "Blob");
    BLOB_PROTOTYPE.with(|slot| *slot.borrow_mut() = Some(prototype.as_value()));
    // `File.prototype` é este mesmo objeto: `install_file` roda depois.
    install_global_with_attributes(global_object, "Blob", constructor.as_value(), 0);
}
