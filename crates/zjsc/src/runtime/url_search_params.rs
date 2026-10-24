//! `URLSearchParams` do global. O JavaScriptCore não o define: quem o instala é o bun (WebCore), como propriedade de
//! dados `writable`, `configurable` e NÃO enumerável. Medido no bun 1.4.2 (`scripts/gen-url-search-params-golden.js`):
//!
//! - construtor nativo `length` 0, `name` "URLSearchParams", chaves próprias `length`, `name`, `prototype`;
//! - protótipo, nesta ordem: `constructor` (não enumerável), `append`(2), `delete`(1), `get`(1), `getAll`(1), `has`(1),
//!   `set`(2), `sort`(0), `entries`, `keys`, `values`, `forEach`, `toString`(0), `toJSON`, os acessores `length` (não
//!   enumerável) e `size` (enumerável), `Symbol.iterator`, `Symbol(nodejs.util.inspect.custom)` e `@@toStringTag`;
//! - sem `new`: ``Use `new URLSearchParams(...)` instead of `URLSearchParams(...)` `` (`ERR_ILLEGAL_CONSTRUCTOR`);
//!   argumento ausente num método: `Not enough arguments` (`ERR_MISSING_ARGS`); `this` alheio:
//!   `Can only call URLSearchParams.<método> on instances of URLSearchParams` (`ERR_INVALID_THIS`);
//! - serialização `application/x-www-form-urlencoded`: sobrevivem ASCII alfanumérico e `*-._`, espaço vira `+`, o resto
//!   é `%XX` do UTF-8 (unidade substituta solta vira U+FFFD); a análise ignora segmentos vazios, tira um `?` inicial,
//!   troca `+` por espaço e decodifica `%XX` (UTF-8 com substituição).
//!
//! Também medido: o construtor com objeto (registro: chaves próprias enumeráveis, símbolo lança `Cannot convert a symbol
//! to a string`) ou com iterável de pares (`Value is not a sequence` `ERR_INVALID_ARG_TYPE` para item que não é objeto,
//! `Type error` para par que não tem dois elementos); `delete(name, value)` e `has(name, value)` (valor `undefined`
//! conta como ausente); `forEach` (`Cannot call callback on a non-function`); `toJSON` (nome repetido vira array, e o
//! objeto leva `@@toStringTag` próprio); `length` devolve o mesmo que `size`; o iterador `URLSearchParams Iterator`
//! (índice vivo sobre a lista, `next` enumerável) e o inspect `URLSearchParams { 'a' => '1' }`.
//! O estado das instâncias e o esqueleto de iterador, `forEach` e brand check são os de `web_iterable.rs`.

use crate::host_function;

use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::derived_structure;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::native_class_support::{create_native_class_with_length, install_global_with_attributes, instance_structure, put_native_accessor, throw_coded_type_error};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::symbol::Symbol;
use crate::runtime::web_iterable::{
    install_iteration, pairs_of_object, pairs_to_json_object, put_enumerable_methods, put_iterator_alias, register_instance, require_arguments, string_value, usv_units_of, with_entries,
    with_instance, Pairs, Units, WebIterable,
};
use crate::web_iterable_functions;
use crate::wtf::text::string_impl::StringImpl;
use crate::wtf::text::wtf_string::String as WtfString;

/// A classe `URLSearchParams` no esqueleto compartilhado.
struct UrlSearchParams;

impl WebIterable for UrlSearchParams {
    const NAME: &'static str = "URLSearchParams";
    const USV_STRINGS: bool = true;

    crate::text_web_iterable_values!();
}

static PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "URLSearchParams", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static ITERATOR_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "URLSearchParams Iterator", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `%XX` e `+` de um segmento já separado, em UTF-8 com substituição.
fn decode_component(segment: &[u8]) -> Units {
    let mut bytes = Vec::with_capacity(segment.len());
    let mut index = 0;
    while index < segment.len() {
        let byte = segment[index];
        let hex = |digit: u8| (digit as char).to_digit(16);
        if byte == b'+' {
            bytes.push(b' ');
        } else if byte == b'%' && index + 2 < segment.len() && hex(segment[index + 1]).is_some() && hex(segment[index + 2]).is_some() {
            bytes.push((hex(segment[index + 1]).unwrap() * 16 + hex(segment[index + 2]).unwrap()) as u8);
            index += 2;
        } else {
            bytes.push(byte);
        }
        index += 1;
    }
    String::from_utf8_lossy(&bytes).encode_utf16().collect()
}

/// A análise de `application/x-www-form-urlencoded` (com `?` inicial removido).
fn parse(input: &[u16]) -> Pairs {
    let text = String::from_utf16_lossy(input);
    parse_query(text.strip_prefix('?').unwrap_or(&text).encode_utf16().collect::<Vec<u16>>().as_slice())
}

/// Os pares de uma cadeia urlencoded, sem tratar o `?` inicial (o corpo de `Blob.formData()` o mantém).
pub(crate) fn parse_query(input: &[u16]) -> Pairs {
    String::from_utf16_lossy(input)
        .split('&')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let (name, value) = segment.split_once('=').unwrap_or((segment, ""));
            (decode_component(name.as_bytes()), decode_component(value.as_bytes()))
        })
        .collect()
}

fn encode_component(units: &[u16], out: &mut String) {
    for byte in String::from_utf16_lossy(units).bytes() {
        match byte {
            b'*' | b'-' | b'.' | b'_' => out.push(byte as char),
            b' ' => out.push('+'),
            _ if byte.is_ascii_alphanumeric() => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
}

/// As raízes do coletor: o protótipo.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.value(PROTOTYPE.with(|prototype| *prototype.borrow()));
}

thread_local! {
    /// O protótipo de `URLSearchParams`, guardado na instalação, para o `URL` criar o `searchParams`.
    static PROTOTYPE: std::cell::RefCell<Option<JSValue>> = const { std::cell::RefCell::new(None) };
}

/// Uma instância nova de `URLSearchParams` com os pares dados (o `searchParams` de um `URL`).
pub(crate) fn create_search_params(global_object: &JSGlobalObject, pairs: Pairs) -> JSValue {
    let prototype = PROTOTYPE.with(|slot| *slot.borrow()).expect("URLSearchParams não instalado");
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype);
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    register_instance::<UrlSearchParams>(instance, pairs);
    instance
}

/// Os pares de `URLSearchParams` de `value`, se for uma instância.
pub(crate) fn pairs_of_instance(value: JSValue) -> Option<Pairs> {
    with_instance::<UrlSearchParams, Pairs>(value, |pairs| pairs.clone())
}

/// Troca a lista de `value` (a atualização que vem do `URL`: `search`, `href`).
pub(crate) fn replace_pairs(value: JSValue, new_pairs: Pairs) {
    with_instance::<UrlSearchParams, ()>(value, |pairs| *pairs = new_pairs);
}

/// A serialização urlencoded dos pares.
pub(crate) fn serialize(pairs: &Pairs) -> String {
    let mut out = String::new();
    for (index, (name, value)) in pairs.iter().enumerate() {
        if index > 0 {
            out.push('&');
        }
        encode_component(name, &mut out);
        out.push('=');
        encode_component(value, &mut out);
    }
    out
}

fn call_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new URLSearchParams(...)` instead of `URLSearchParams(...)`", "ERR_ILLEGAL_CONSTRUCTOR"))
}

/// A lista inicial de `new URLSearchParams(init)`.
fn initial_pairs(global_object: &JSGlobalObject, init: JSValue) -> Result<Pairs, Thrown> {
    if let Some(pairs) = with_instance::<UrlSearchParams, Pairs>(init, |pairs| pairs.clone()) {
        return Ok(pairs);
    }
    if init.is_undefined() {
        return Ok(Vec::new());
    }
    if !init.is_object() {
        return Ok(parse(&usv_units_of(global_object, init)?));
    }
    pairs_of_object::<UrlSearchParams>(global_object, init)
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let pairs = initial_pairs(global_object, call.argument(0))?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    register_instance::<UrlSearchParams>(instance, pairs);
    Ok(instance)
}

fn append_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_entries::<UrlSearchParams, _>(global_object, call, "append", |_| ())?;
    require_arguments(global_object, call, 2)?;
    let (name, value) = (usv_units_of(global_object, call.argument(0))?, usv_units_of(global_object, call.argument(1))?);
    with_entries::<UrlSearchParams, _>(global_object, call, "append", |pairs| pairs.push((name, value)))?;
    crate::runtime::url::search_params_changed(call.this_value());
    Ok(JSValue::undefined())
}

fn set_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_entries::<UrlSearchParams, _>(global_object, call, "set", |_| ())?;
    require_arguments(global_object, call, 2)?;
    let (name, value) = (usv_units_of(global_object, call.argument(0))?, usv_units_of(global_object, call.argument(1))?);
    with_entries::<UrlSearchParams, _>(global_object, call, "set", |pairs| match pairs.iter().position(|pair| pair.0 == name) {
        Some(first) => {
            pairs[first].1 = value;
            let mut seen = 0;
            pairs.retain(|pair| pair.0 != name || { seen += 1; seen == 1 });
        }
        None => pairs.push((name, value)),
    })?;
    crate::runtime::url::search_params_changed(call.this_value());
    Ok(JSValue::undefined())
}

/// O segundo argumento opcional de `delete` e `has`: `undefined` conta como ausente.
fn optional_value(global_object: &JSGlobalObject, call: &HostCall) -> Result<Option<Units>, Thrown> {
    match call.argument(1) {
        value if call.argument_count() < 2 || value.is_undefined() => Ok(None),
        value => usv_units_of(global_object, value).map(Some),
    }
}

fn delete_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_entries::<UrlSearchParams, _>(global_object, call, "delete", |_| ())?;
    require_arguments(global_object, call, 1)?;
    let name = usv_units_of(global_object, call.argument(0))?;
    let value = optional_value(global_object, call)?;
    with_entries::<UrlSearchParams, _>(global_object, call, "delete", |pairs| pairs.retain(|pair| pair.0 != name || value.as_ref().is_some_and(|value| pair.1 != *value)))?;
    crate::runtime::url::search_params_changed(call.this_value());
    Ok(JSValue::undefined())
}

fn get_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_entries::<UrlSearchParams, _>(global_object, call, "get", |_| ())?;
    require_arguments(global_object, call, 1)?;
    let name = usv_units_of(global_object, call.argument(0))?;
    let found = with_entries::<UrlSearchParams, _>(global_object, call, "get", |pairs| pairs.iter().find(|pair| pair.0 == name).map(|pair| pair.1.clone()))?;
    Ok(found.map_or(JSValue::null(), |value| string_value(global_object, &value)))
}

fn get_all_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_entries::<UrlSearchParams, _>(global_object, call, "getAll", |_| ())?;
    require_arguments(global_object, call, 1)?;
    let name = usv_units_of(global_object, call.argument(0))?;
    let values = with_entries::<UrlSearchParams, _>(global_object, call, "getAll", |pairs| pairs.iter().filter(|pair| pair.0 == name).map(|pair| pair.1.clone()).collect::<Vec<_>>())?;
    let values: Vec<JSValue> = values.iter().map(|value| string_value(global_object, value)).collect();
    Ok(construct_array(global_object.vm(), &global_object.array_structure(), &values).as_value())
}

fn has_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_entries::<UrlSearchParams, _>(global_object, call, "has", |_| ())?;
    require_arguments(global_object, call, 1)?;
    let name = usv_units_of(global_object, call.argument(0))?;
    let value = optional_value(global_object, call)?;
    let found = with_entries::<UrlSearchParams, _>(global_object, call, "has", |pairs| pairs.iter().any(|pair| pair.0 == name && value.as_ref().is_none_or(|value| pair.1 == *value)))?;
    Ok(JSValue::Bool(found))
}

fn sort_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_entries::<UrlSearchParams, _>(global_object, call, "sort", |pairs| pairs.sort_by(|left, right| left.0.cmp(&right.0)))?;
    crate::runtime::url::search_params_changed(call.this_value());
    Ok(JSValue::undefined())
}

fn to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let text = with_entries::<UrlSearchParams, _>(global_object, call, "toString", |pairs| serialize(pairs))?;
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(text.as_bytes()))))
}

fn size_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let count = with_entries::<UrlSearchParams, _>(global_object, call, "size", |pairs| pairs.len())?;
    Ok(js_number(count as f64))
}

/// O objeto de `toJSON` de `value` quando é um `URLSearchParams`: o que o `console.log` imprime.
pub(crate) fn json_object_of(global_object: &JSGlobalObject, value: JSValue) -> Option<JSValue> {
    crate::runtime::web_iterable::json_object_of::<UrlSearchParams>(global_object, value)
}

/// `toJSON`: nome repetido vira array; o objeto leva um `@@toStringTag` próprio (medido, vaza do bun).
fn to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let pairs = with_entries::<UrlSearchParams, _>(global_object, call, "toJSON", |pairs| pairs.clone())?;
    Ok(pairs_to_json_object::<UrlSearchParams>(global_object, pairs, Some("URLSearchParams")))
}

/// A cadeia como o `util.inspect` do Node a cita: aspas simples, ou duplas, ou crase conforme o conteúdo.
pub(crate) fn quote_units(units: &[u16]) -> String {
    let chars: Vec<Result<char, u16>> = char::decode_utf16(units.iter().copied()).map(|unit| unit.map_err(|error| error.unpaired_surrogate())).collect();
    let has = |wanted: char| chars.iter().any(|item| *item == Ok(wanted));
    let quote = if !has('\'') {
        '\''
    } else if !has('"') {
        '"'
    } else if !has('`') && !has('$') {
        '`'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for item in chars {
        match item {
            Ok('\n') => out.push_str("\\n"),
            Ok('\t') => out.push_str("\\t"),
            Ok('\r') => out.push_str("\\r"),
            Ok('\u{8}') => out.push_str("\\b"),
            Ok('\u{c}') => out.push_str("\\f"),
            Ok('\u{b}') => out.push_str("\\v"),
            Ok('\\') => out.push_str("\\\\"),
            Ok(character) if character == quote => {
                out.push('\\');
                out.push(character);
            }
            Ok(character) if (character as u32) < 0x20 || character as u32 == 0x7f => out.push_str(&format!("\\x{:02X}", character as u32)),
            Ok(character) => out.push(character),
            Err(unit) => out.push_str(&format!("\\u{unit:04X}")),
        }
    }
    out.push(quote);
    out
}

/// `[Symbol.for('nodejs.util.inspect.custom')](depth, options)`: o texto de `util.inspect`; `this` alheio volta como está.
fn inspect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = call.this_value();
    let Some(pairs) = with_instance::<UrlSearchParams, Pairs>(this_value, |pairs| pairs.clone()) else { return Ok(this_value) };
    let depth = call.argument(0);
    let text = if depth.is_number() && depth.as_number() < 0.0 {
        "[Object]".to_string()
    } else {
        inspect_text(&pairs)
    };
    let units: Units = text.encode_utf16().collect();
    Ok(string_value(global_object, &units))
}

/// O texto de `util.inspect` de um `URLSearchParams` com a lista `pairs`.
pub(crate) fn inspect_text(pairs: &Pairs) -> String {
    if pairs.is_empty() {
        return "URLSearchParams {}".to_string();
    }
    let entries: Vec<String> = pairs.iter().map(|(name, value)| format!("{} => {}", quote_units(name), quote_units(value))).collect();
    format!("URLSearchParams {{ {} }}", entries.join(", "))
}

host_function!(call_fn, call_body);
host_function!(construct_fn, construct_body);
host_function!(append_fn, append_body);
host_function!(set_fn, set_body);
host_function!(delete_fn, delete_body);
host_function!(get_fn, get_body);
host_function!(get_all_fn, get_all_body);
host_function!(has_fn, has_body);
host_function!(sort_fn, sort_body);
host_function!(to_string_fn, to_string_body);
host_function!(size_fn, size_body);
host_function!(to_json_fn, to_json_body);
host_function!(inspect_fn, inspect_body);
web_iterable_functions!(UrlSearchParams);

/// Instala `URLSearchParams` no global.
pub fn install_url_search_params(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let (prototype, constructor) = create_native_class_with_length(global_object, &PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "URLSearchParams", 0, call_fn, construct_fn);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    let methods: [(&str, u32, NativeFunction); 13] = [
        ("append", 2, append_fn),
        ("delete", 1, delete_fn),
        ("get", 1, get_fn),
        ("getAll", 1, get_all_fn),
        ("has", 1, has_fn),
        ("set", 2, set_fn),
        ("sort", 0, sort_fn),
        ("entries", 0, entries_fn),
        ("keys", 0, keys_fn),
        ("values", 0, values_fn),
        ("forEach", 1, for_each_fn),
        ("toString", 0, to_string_fn),
        ("toJSON", 0, to_json_fn),
    ];
    put_enumerable_methods(global_object, &prototype, &methods);
    // Atributos do WebCore (`CustomAccessor`): o `toString()` do getter é o nome puro (`function size() { [native code] }`),
    // medido no bun 1.4.2, ao contrário do getter intrínseco do JSC (`function get byteLength() { ... }`).
    put_native_accessor(vm, global_object, &prototype, "length", size_fn, None, DONT_ENUM);
    put_native_accessor(vm, global_object, &prototype, "size", size_fn, None, 0);
    put_iterator_alias(global_object, &prototype);
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
    install_iteration::<UrlSearchParams>(global_object, &prototype, &ITERATOR_PROTOTYPE_S_INFO, iterator_next_fn);
    PROTOTYPE.with(|slot| *slot.borrow_mut() = Some(prototype.as_value()));
    install_global_with_attributes(global_object, "URLSearchParams", constructor.as_value(), DONT_ENUM);
}
