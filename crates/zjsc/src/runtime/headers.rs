//! `Headers` do global. O JavaScriptCore não o define: quem o instala é o bun (WebCore), como propriedade de dados
//! `writable`, `configurable` e ENUMERÁVEL (ao contrário de `URLSearchParams`). Medido no bun 1.4.2
//! (`scripts/gen-headers-golden.js`):
//!
//! - construtor nativo `length` 0, `name` "Headers"; protótipo, nesta ordem: `constructor` (não enumerável), `append`(2),
//!   `delete`(1), `get`(1), `getAll`(1), `has`(1), `set`(2), `entries`, `keys`, `values`, `forEach`(1), `toJSON`, o
//!   acessor `count` (enumerável), `getSetCookie`, `Symbol.iterator` (a própria `entries`) e `@@toStringTag`;
//! - sem `new`: ``Use `new Headers(...)` instead of `Headers(...)` `` (`ERR_ILLEGAL_CONSTRUCTOR`); `this` alheio:
//!   `Can only call Headers.<método> on instances of Headers` (`ERR_INVALID_THIS`), exceto `getSetCookie` e `count`, que
//!   devolvem `undefined`; argumento ausente: `Not enough arguments` (`ERR_MISSING_ARGS`);
//! - nome: minúsculo, só caracteres de `token` (senão `Invalid header name: '<nome>'`, e só o `has` fecha com `"`, como o bun); valor: aparado de `\t \n \r` e espaço, sem NUL, CR, LF nem unidade acima de 0xFF
//!   (senão `Header '<nome>' has invalid value: '<valor>'`);
//! - nomes repetidos combinam com `, ` (o resto fica na posição do primeiro); `set-cookie` nunca combina: cada valor é
//!   uma entrada, e `get` os junta com `, `. Iteração: os nomes que não são `set-cookie` em ordem crescente, depois os
//!   cookies na ordem de chegada; o iterador e `forEach` releem essa lista a cada passo;
//! - construtor: `undefined` vazio; outro `Headers` copia; objeto com `Symbol.iterator` é sequência de pares (item não
//!   objeto `Value is not a sequence`, par que não tem dois elementos `Header sub-sequence must contain exactly two
//!   items`); objeto sem iterador (função inclusive) é registro; o resto é `Type error`;
//! - `getAll` só aceita `set-cookie` (`Only "set-cookie" is supported.`, sem argumento `Missing argument`), e num
//!   `this` que não é `Headers` devolve `undefined`; `toJSON`
//!   põe `set-cookie` (array) primeiro e o resto na ordem de inserção; `count` (acessor enumerável e não configurável)
//!   é o número de entradas da iteração.
//! O estado das instâncias fica em `thread_local` (zerado em `reset_for_program`), como `URLSearchParams`.

use std::borrow::Cow;
use std::cell::RefCell;

use crate::host_function;
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::derived_structure;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::native_class_support::{create_native_class_with_length, install_global_with_attributes, instance_structure, throw_coded_type_error};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM};
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::web_iterable::{
    install_iteration, pairs_of_object, put_enumerable_methods, put_iterator_alias, register_instance, require_arguments, string_value, units_of, with_entries, with_instance,
    Pairs as Entries, Units, WebIterable,
};
use crate::web_iterable_functions;
use crate::wtf::text::wtf_string::String as WtfString;

/// A classe `Headers` no esqueleto compartilhado: iteração ordenada, par validado na gravação.
struct Headers;

impl WebIterable for Headers {
    const NAME: &'static str = "Headers";
    const ITEM_NEEDS_ITERATOR: bool = true;
    const SUB_SEQUENCE_ERROR: &'static str = "Header sub-sequence must contain exactly two items";

    crate::text_web_iterable_values!();

    fn ordered(entries: &Entries) -> Cow<'_, Entries> {
        Cow::Owned(sorted(entries))
    }

    fn push(entries: &mut Entries, name: Units, value: Units) -> Result<(), Thrown> {
        let normalized_name = normalize_name(&name, '\'')?;
        let normalized_value = normalize_value(&name, &value)?;
        append_entry(entries, normalized_name, normalized_value);
        Ok(())
    }
}

static PROTOTYPE_S_INFO: ClassInfo = ClassInfo { class_name: "Headers", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static ITERATOR_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Headers Iterator", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

const SET_COOKIE: &str = "set-cookie";

fn units_of_str(text: &str) -> Units {
    text.encode_utf16().collect()
}

fn is_set_cookie(name: &[u16]) -> bool {
    name == units_of_str(SET_COOKIE).as_slice()
}

fn is_token_unit(unit: u16) -> bool {
    unit < 0x80 && ((unit as u8).is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&(unit as u8)))
}

fn is_http_space(unit: u16) -> bool {
    matches!(unit, 9 | 10 | 13 | 32)
}

fn trim_http(units: &[u16]) -> Units {
    let start = units.iter().position(|unit| !is_http_space(*unit)).unwrap_or(units.len());
    let end = units.iter().rposition(|unit| !is_http_space(*unit)).map_or(start, |last| last + 1);
    units[start..end].to_vec()
}

/// O nome validado e em minúsculas.
fn normalize_name(name: &[u16], closing: char) -> Result<Units, Thrown> {
    if name.is_empty() || !name.iter().all(|unit| is_token_unit(*unit)) {
        return Err(Thrown::type_error(&format!("Invalid header name: '{}{closing}", String::from_utf16_lossy(name))));
    }
    Ok(lowercase(name))
}

fn lowercase(name: &[u16]) -> Units {
    name.iter().map(|unit| if *unit < 0x80 { (*unit as u8).to_ascii_lowercase() as u16 } else { *unit }).collect()
}

/// O valor aparado e validado; `name` é o nome como veio, para a mensagem.
fn normalize_value(name: &[u16], value: &[u16]) -> Result<Units, Thrown> {
    let trimmed = trim_http(value);
    if trimmed.iter().any(|unit| matches!(unit, 0 | 10 | 13) || *unit > 0xFF) {
        return Err(Thrown::type_error(&format!("Header '{}' has invalid value: '{}'", String::from_utf16_lossy(name), String::from_utf16_lossy(&trimmed))));
    }
    Ok(trimmed)
}

fn join_comma(parts: impl Iterator<Item = Units>) -> Units {
    let mut out = Units::new();
    for (index, part) in parts.enumerate() {
        if index > 0 {
            out.extend_from_slice(&units_of_str(", "));
        }
        out.extend(part);
    }
    out
}

fn append_entry(entries: &mut Entries, name: Units, value: Units) {
    if !is_set_cookie(&name) {
        if let Some(existing) = entries.iter_mut().find(|entry| entry.0 == name) {
            existing.1.extend_from_slice(&units_of_str(", "));
            existing.1.extend(value);
            return;
        }
    }
    entries.push((name, value));
}

fn set_entry(entries: &mut Entries, name: Units, value: Units) {
    match entries.iter().position(|entry| entry.0 == name) {
        Some(first) if !is_set_cookie(&name) => entries[first].1 = value,
        _ => {
            entries.retain(|entry| entry.0 != name);
            entries.push((name, value));
        }
    }
}

/// A ordem da iteração: nomes comuns crescentes, depois os cookies na ordem de chegada.
fn sorted(entries: &Entries) -> Entries {
    let mut plain: Entries = entries.iter().filter(|entry| !is_set_cookie(&entry.0)).cloned().collect();
    plain.sort_by(|left, right| left.0.cmp(&right.0));
    plain.extend(entries.iter().filter(|entry| is_set_cookie(&entry.0)).cloned());
    plain
}

fn call_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new Headers(...)` instead of `Headers(...)`", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn initial_entries(global_object: &JSGlobalObject, init: JSValue) -> Result<Entries, Thrown> {
    if let Some(entries) = with_instance::<Headers, Entries>(init, |entries| entries.clone()) {
        return Ok(entries);
    }
    if init.is_undefined() {
        return Ok(Vec::new());
    }
    if !init.is_object() {
        return Err(Thrown::type_error("Type error"));
    }
    pairs_of_object::<Headers>(global_object, init)
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let entries = initial_entries(global_object, call.argument(0))?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    register_instance::<Headers>(instance, entries);
    Ok(instance)
}

/// As raízes do coletor: o protótipo.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.value(HEADERS_PROTOTYPE.with(|prototype| *prototype.borrow()));
}

thread_local! {
    /// O protótipo de `Headers`, guardado na instalação para os `Headers` que `Response` cria.
    static HEADERS_PROTOTYPE: RefCell<Option<JSValue>> = const { RefCell::new(None) };
}

fn new_headers(global_object: &JSGlobalObject, entries: Entries) -> JSValue {
    let prototype = HEADERS_PROTOTYPE.with(|slot| slot.borrow().expect("Headers sem protótipo (instalação)"));
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype);
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    register_instance::<Headers>(instance, entries);
    instance
}

/// Um `Headers` novo a partir do `init` de `new Headers(init)` (o `headers` de um `ResponseInit`).
pub(crate) fn make_headers(global_object: &JSGlobalObject, init: JSValue) -> HostResult {
    Ok(new_headers(global_object, initial_entries(global_object, init)?))
}

/// Uma cópia independente de `headers` (o `clone` de `Response`).
pub(crate) fn clone_headers(global_object: &JSGlobalObject, headers: JSValue) -> JSValue {
    new_headers(global_object, with_instance::<Headers, Entries>(headers, |entries| entries.clone()).unwrap_or_default())
}

/// `headers` não tem nenhuma entrada (o `headers: {}` de `new Request` não substitui os herdados).
pub(crate) fn headers_is_empty(headers: JSValue) -> bool {
    with_instance::<Headers, bool>(headers, |entries| entries.is_empty()).unwrap_or(true)
}

/// O valor de `name` (já em minúsculas) em `headers`, como `get`: os valores combinados com `, `.
pub(crate) fn header_value(headers: JSValue, name: &str) -> Option<Vec<u8>> {
    let name = units_of_str(name);
    let (_, value) = combined_headers(headers)?.into_iter().find(|entry| entry.0 == name)?;
    Some(value.into_iter().map(|unit| unit as u8).collect())
}

/// As entradas como o `HTTPHeaderMap` do `FetchHeaders` as guarda: um par por nome, na ordem em que o nome apareceu,
/// com os valores combinados por `, ` (o `size()` e o `get()` que o `Bun.deepEquals` compara).
pub(crate) fn combined_headers(headers: JSValue) -> Option<Vec<(Units, Units)>> {
    with_instance::<Headers, Vec<(Units, Units)>>(headers, |entries| {
        let mut names: Vec<Units> = Vec::new();
        for entry in entries.iter() {
            if !names.contains(&entry.0) {
                names.push(entry.0.clone());
            }
        }
        names
            .into_iter()
            .map(|name| {
                let value = join_comma(entries.iter().filter(|entry| entry.0 == name).map(|entry| entry.1.clone()));
                (name, value)
            })
            .collect()
    })
}

/// Acrescenta `name: value` a `headers` se o nome ainda não existe (o `Content-Type` implícito do corpo).
pub(crate) fn append_default_header(headers: JSValue, name: &str, value: &[u8]) {
    let name = units_of_str(name);
    with_instance::<Headers, ()>(headers, |entries| {
        if !entries.iter().any(|entry| entry.0 == name) {
            append_entry(entries, name, value.iter().map(|byte| u16::from(*byte)).collect());
        }
    });
}

/// Tira `name` (já em minúsculas) de `headers`, se existir (o `Content-Type` implícito de um corpo de `fetch` lido).
pub(crate) fn remove_header(headers: JSValue, name: &str) {
    let name = units_of_str(name);
    with_instance::<Headers, ()>(headers, |entries| entries.retain(|entry| entry.0 != name));
}

/// `append` e `set`: a ordem é `this`, argumentos, conversão do nome e do valor, validação do nome, do valor.
fn write_entry(global_object: &JSGlobalObject, call: &HostCall, method: &str, write: fn(&mut Entries, Units, Units)) -> HostResult {
    with_entries::<Headers, _>(global_object, call, method, |_| ())?;
    require_arguments(global_object, call, 2)?;
    let (name, value) = (units_of(global_object, call.argument(0))?, units_of(global_object, call.argument(1))?);
    let (normalized_name, normalized_value) = (normalize_name(&name, '\'')?, normalize_value(&name, &value)?);
    with_entries::<Headers, _>(global_object, call, method, |entries| write(entries, normalized_name, normalized_value))?;
    Ok(JSValue::undefined())
}

fn append_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    write_entry(global_object, call, "append", append_entry)
}

fn set_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    write_entry(global_object, call, "set", set_entry)
}

/// O nome do primeiro argumento de `delete`, `get` e `has`, validado e em minúsculas.
fn name_argument(global_object: &JSGlobalObject, call: &HostCall, method: &str) -> Result<Units, Thrown> {
    with_entries::<Headers, _>(global_object, call, method, |_| ())?;
    require_arguments(global_object, call, 1)?;
    // O `has` do bun fecha a mensagem com `"` em vez de `'` (medido).
    normalize_name(&units_of(global_object, call.argument(0))?, if method == "has" { '"' } else { '\'' })
}

fn delete_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = name_argument(global_object, call, "delete")?;
    with_entries::<Headers, _>(global_object, call, "delete", |entries| entries.retain(|entry| entry.0 != name))?;
    Ok(JSValue::undefined())
}

fn get_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = name_argument(global_object, call, "get")?;
    let found = with_entries::<Headers, _>(global_object, call, "get", |entries| {
        let values: Vec<Units> = entries.iter().filter(|entry| entry.0 == name).map(|entry| entry.1.clone()).collect();
        (!values.is_empty()).then(|| join_comma(values.into_iter()))
    })?;
    Ok(found.map_or(JSValue::null(), |value| string_value(global_object, &value)))
}

fn cookies_array(global_object: &JSGlobalObject, entries: &Entries) -> JSValue {
    let values: Vec<JSValue> = entries.iter().filter(|entry| is_set_cookie(&entry.0)).map(|entry| string_value(global_object, &entry.1)).collect();
    construct_array(global_object.vm(), &global_object.array_structure(), &values).as_value()
}

fn get_all_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Medido no bun 1.4.2: num `this` que não é `Headers` o `getAll` devolve `undefined`, sem conferir os argumentos.
    let Some(entries) = with_instance::<Headers, Entries>(call.this_value(), |entries| entries.clone()) else {
        return Ok(JSValue::undefined());
    };
    if call.argument_count() < 1 {
        return Err(Thrown::type_error("Missing argument"));
    }
    if !is_set_cookie(&lowercase(&units_of(global_object, call.argument(0))?)) {
        return Err(Thrown::type_error("Only \"set-cookie\" is supported."));
    }
    Ok(cookies_array(global_object, &entries))
}

fn has_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = name_argument(global_object, call, "has")?;
    let found = with_entries::<Headers, _>(global_object, call, "has", |entries| entries.iter().any(|entry| entry.0 == name))?;
    Ok(JSValue::Bool(found))
}

/// `getSetCookie`: `this` alheio devolve `undefined`.
fn get_set_cookie_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(with_instance::<Headers, _>(call.this_value(), |entries| cookies_array(global_object, entries)).unwrap_or(JSValue::undefined()))
}

fn count_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(with_instance::<Headers, _>(call.this_value(), |entries| js_number(entries.len() as f64)).unwrap_or(JSValue::undefined()))
}

fn to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let entries = with_entries::<Headers, _>(global_object, call, "toJSON", |entries| entries.clone())?;
    Ok(json_object(global_object, &entries))
}

/// O objeto de `toJSON` de `value` quando é um `Headers`: o que o `console.log` imprime.
pub(crate) fn json_object_of(global_object: &JSGlobalObject, value: JSValue) -> Option<JSValue> {
    with_instance::<Headers, _>(value, |entries| json_object(global_object, entries))
}

/// `set-cookie` (array) primeiro e o resto na ordem de inserção.
fn json_object(global_object: &JSGlobalObject, entries: &Entries) -> JSValue {
    let vm = global_object.vm();
    let object = JSFinalObject::create(vm, &global_object.object_structure_for_object_constructor());
    let define = |name: &[u16], value: JSValue| {
        let key = PropertyName::from_identifier(&Identifier::from_string(vm, &WtfString::from_utf16(name)));
        let _ = object.define_own_property(vm, &key, &PropertyDescriptor::new(value, 0), false);
    };
    if entries.iter().any(|entry| is_set_cookie(&entry.0)) {
        define(&units_of_str(SET_COOKIE), cookies_array(global_object, entries));
    }
    for (name, value) in entries.iter().filter(|entry| !is_set_cookie(&entry.0)) {
        define(name, string_value(global_object, value));
    }
    object.as_value()
}

host_function!(call_fn, call_body);
host_function!(construct_fn, construct_body);
host_function!(append_fn, append_body);
host_function!(set_fn, set_body);
host_function!(delete_fn, delete_body);
host_function!(get_fn, get_body);
host_function!(get_all_fn, get_all_body);
host_function!(has_fn, has_body);
host_function!(get_set_cookie_fn, get_set_cookie_body);
host_function!(count_fn, count_body);
host_function!(to_json_fn, to_json_body);
web_iterable_functions!(Headers);

/// Instala `Headers` no global.
pub fn install_headers(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let (prototype, constructor) = create_native_class_with_length(global_object, &PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "Headers", 0, call_fn, construct_fn);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    HEADERS_PROTOTYPE.with(|slot| *slot.borrow_mut() = Some(prototype.as_value()));
    let methods: [(&str, u32, NativeFunction); 11] = [
        ("append", 2, append_fn),
        ("delete", 1, delete_fn),
        ("get", 1, get_fn),
        ("getAll", 1, get_all_fn),
        ("has", 1, has_fn),
        ("set", 2, set_fn),
        ("entries", 0, entries_fn),
        ("keys", 0, keys_fn),
        ("values", 0, values_fn),
        ("forEach", 1, for_each_fn),
        ("toJSON", 0, to_json_fn),
    ];
    put_enumerable_methods(global_object, &prototype, &methods);
    put_native_getter(vm, global_object, &prototype, "count", count_fn, Intrinsic::NoIntrinsic, DONT_DELETE);
    let set_cookie_methods: [(&str, u32, NativeFunction); 1] = [("getSetCookie", 0, get_set_cookie_fn)];
    put_enumerable_methods(global_object, &prototype, &set_cookie_methods);
    put_iterator_alias(global_object, &prototype);
    install_iteration::<Headers>(global_object, &prototype, &ITERATOR_PROTOTYPE_S_INFO, iterator_next_fn);
    install_global_with_attributes(global_object, "Headers", constructor.as_value(), 0);
}
