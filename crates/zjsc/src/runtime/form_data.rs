//! `FormData` do global. O JavaScriptCore não o define: quem o instala é o bun (WebCore). Medido no bun 1.4.2
//! (`scripts/gen-file-formdata-golden.js`, `tests/golden/file_formdata_bun.tsv`):
//!
//! - propriedade global de dados `writable`, `configurable` e ENUMERÁVEL (ao contrário de `URLSearchParams`);
//! - construtor nativo `length` 0, `name` "FormData"; o argumento é ignorado (`new FormData(form)` sai vazio);
//! - protótipo, nesta ordem: `constructor`, `append`(2), `delete`(1), `get`(1), `getAll`(1), `has`(1), `set`(2),
//!   `entries`, `keys`, `values`, `forEach`(1), `toJSON`(1, `writable` e `configurable` falsos) e o acessor `length`
//!   (não enumerável, não configurável); `Symbol.iterator` é `entries` e `@@toStringTag` é "FormData";
//! - nome e valor são `USVString` (unidade substituta solta vira U+FFFD, na gravação e na busca); `delete` e `has`
//!   olham só o nome; o iterador `FormData Iterator` é vivo e, esgotado, continua esgotado;
//! - erros: sem `new`, ``Use `new FormData(...)` instead of `FormData(...)` `` (`ERR_ILLEGAL_CONSTRUCTOR`); argumento
//!   ausente, `Not enough arguments`; `this` alheio, `Can only call FormData.<método> on instances of FormData`
//!   (`ERR_INVALID_THIS`), e o getter `length` lança `Type error` simples.
//!
//! O ramo Blob/File (`append`/`set` com `Blob` ou `File`): a entrada guarda uma cópia do estado ([`FormValue::Blob`]),
//! cada `get`/`getAll`/iteração devolve um `Blob` novo (nunca o objeto original, medido) já marcado como arquivo
//! (`instanceof File`), com o nome do `File` (o terceiro argumento é ignorado) ou o terceiro argumento para um `Blob`.
//! `FormData.from` segue o `FormData__jsFunctionFromMultipartData` do bun; o corpo multipart de `Response`/`Request` fica
//! para quando elas existirem.
//! O iterador vivo, o `forEach` e o `toJSON` são os de `web_iterable.rs`.

use std::cell::RefCell;

use crate::host_function;
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::blob::{binary_bytes, make_blob, state_of, string_bytes, BlobState};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::text_decoder::input_bytes;
use crate::runtime::form_data_parse::{parse_with_boundary, Entry};
use crate::runtime::js_module_loader::throw_error;
use crate::runtime::collection_support::derived_structure;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::native_class_support::{create_native_class_with_length, install_global_with_attributes, instance_structure, throw_coded_type_error};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::web_iterable::{
    install_iteration, pairs_to_json_object, put_enumerable_methods, put_iterator_alias, register_instance, require_arguments, string_value, usv_units_of, with_entries, with_instance, Units,
    WebIterable,
};
use crate::web_iterable_functions;

/// A classe `FormData` no esqueleto compartilhado.
struct FormData;

/// O valor de uma entrada: texto, ou a cópia do estado de um `Blob`/`File` (cada leitura constrói um `Blob` novo).
#[derive(Clone)]
pub(crate) enum FormValue {
    Text(Units),
    Blob(BlobState),
}

impl WebIterable for FormData {
    const NAME: &'static str = "FormData";
    const USV_STRINGS: bool = true;
    type Value = FormValue;

    fn text(units: Units) -> FormValue {
        FormValue::Text(units)
    }

    fn to_js(global_object: &JSGlobalObject, value: &FormValue) -> JSValue {
        match value {
            FormValue::Text(units) => string_value(global_object, units),
            FormValue::Blob(state) => make_blob(global_object, state.clone()),
        }
    }
}

static PROTOTYPE_S_INFO: ClassInfo = ClassInfo { class_name: "FormData", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static ITERATOR_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "FormData Iterator", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `USVString`: a conversão para cadeia e a troca de unidade substituta solta por U+FFFD.
fn usv(global_object: &JSGlobalObject, value: JSValue) -> Result<Units, Thrown> {
    Ok(String::from_utf16_lossy(&usv_units_of(global_object, value)?).encode_utf16().collect())
}

/// As raízes do coletor: o protótipo.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.value(FORM_DATA_PROTOTYPE.with(|prototype| *prototype.borrow()));
}

thread_local! {
    /// O protótipo de `FormData`, guardado na instalação para os `FormData` que `Blob.formData()` cria.
    static FORM_DATA_PROTOTYPE: RefCell<Option<JSValue>> = const { RefCell::new(None) };
}

/// Um `FormData` novo com as entradas do corpo de um `Blob`: o arquivo leva o nome (vazio não dá nome), `lastModified` 0.
pub(crate) fn make_form_data(global_object: &JSGlobalObject, entries: Vec<Entry>) -> JSValue {
    let prototype = FORM_DATA_PROTOTYPE.with(|slot| slot.borrow().expect("FormData sem protótipo (instalação)"));
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype);
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    let pairs = entries
        .into_iter()
        .map(|entry| match entry {
            Entry::Text(name, value) => (name, FormValue::Text(value)),
            Entry::File { name, filename, content_type, bytes } => {
                let state = BlobState { bytes, content_type, name: (!filename.is_empty()).then_some(filename), is_file: true, last_modified: Some(0.0) };
                (name, FormValue::Blob(state))
            }
        })
        .collect();
    register_instance::<FormData>(instance, pairs);
    instance
}

/// As entradas de `value`, se for um `FormData` (o corpo multipart de `Response`).
pub(crate) fn entries_of_instance(value: JSValue) -> Option<Vec<(Units, FormValue)>> {
    with_instance::<FormData, Vec<(Units, FormValue)>>(value, |pairs| pairs.clone())
}

fn call_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new FormData(...)` instead of `FormData(...)`", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    register_instance::<FormData>(instance, Vec::new());
    Ok(instance)
}

/// O estado que o `FormData` guarda para um `Blob`/`File` (`append`/`set`): sempre um arquivo. Um `File` mantém nome e
/// `lastModified` (o terceiro argumento é ignorado); um `Blob` ganha o terceiro argumento como nome (`undefined` e
/// vazio não dão nome) e `lastModified` 0.
fn file_state(global_object: &JSGlobalObject, call: &HostCall, mut state: BlobState) -> Result<BlobState, Thrown> {
    if !state.is_file {
        state.last_modified = Some(0.0);
        let file_name = call.argument(2);
        if call.argument_count() >= 3 && !file_name.is_undefined() {
            let name = usv_units_of(global_object, file_name)?;
            state.name = if name.is_empty() { None } else { Some(name) };
        }
        state.is_file = true;
    }
    Ok(state)
}

/// Nome e valor de `append`/`set`: um `Blob`/`File` vira uma entrada de arquivo; qualquer outro valor é texto, e com
/// o terceiro argumento (nome de arquivo) é `TypeError`.
fn name_and_value(global_object: &JSGlobalObject, call: &HostCall, method: &str) -> Result<(Units, FormValue), Thrown> {
    with_entries::<FormData, _>(global_object, call, method, |_| ())?;
    require_arguments(global_object, call, 2)?;
    let name = usv(global_object, call.argument(0))?;
    if let Some(state) = state_of(call.argument(1)) {
        return Ok((name, FormValue::Blob(file_state(global_object, call, state)?)));
    }
    if call.argument_count() >= 3 {
        return Err(Thrown::type_error("Expected argument to be a Blob."));
    }
    Ok((name, FormValue::Text(usv(global_object, call.argument(1))?)))
}

fn append_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let entry = name_and_value(global_object, call, "append")?;
    with_entries::<FormData, _>(global_object, call, "append", |pairs| pairs.push(entry))?;
    Ok(JSValue::undefined())
}

fn set_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (name, value) = name_and_value(global_object, call, "set")?;
    with_entries::<FormData, _>(global_object, call, "set", |pairs| match pairs.iter().position(|pair| pair.0 == name) {
        Some(first) => {
            pairs[first].1 = value;
            let mut seen = 0;
            pairs.retain(|pair| pair.0 != name || { seen += 1; seen == 1 });
        }
        None => pairs.push((name, value)),
    })?;
    Ok(JSValue::undefined())
}

/// O nome (primeiro argumento obrigatório) de `delete`, `get`, `getAll` e `has`.
fn name_argument(global_object: &JSGlobalObject, call: &HostCall, method: &str) -> Result<Units, Thrown> {
    with_entries::<FormData, _>(global_object, call, method, |_| ())?;
    require_arguments(global_object, call, 1)?;
    usv(global_object, call.argument(0))
}

fn delete_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = name_argument(global_object, call, "delete")?;
    with_entries::<FormData, _>(global_object, call, "delete", |pairs| pairs.retain(|pair| pair.0 != name))?;
    Ok(JSValue::undefined())
}

fn get_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = name_argument(global_object, call, "get")?;
    let found = with_entries::<FormData, _>(global_object, call, "get", |pairs| pairs.iter().find(|pair| pair.0 == name).map(|pair| pair.1.clone()))?;
    Ok(found.map_or(JSValue::null(), |value| FormData::to_js(global_object, &value)))
}

fn get_all_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = name_argument(global_object, call, "getAll")?;
    let values = with_entries::<FormData, _>(global_object, call, "getAll", |pairs| pairs.iter().filter(|pair| pair.0 == name).map(|pair| pair.1.clone()).collect::<Vec<_>>())?;
    let values: Vec<JSValue> = values.iter().map(|value| FormData::to_js(global_object, value)).collect();
    Ok(construct_array(global_object.vm(), &global_object.array_structure(), &values).as_value())
}

fn has_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = name_argument(global_object, call, "has")?;
    Ok(JSValue::Bool(with_entries::<FormData, _>(global_object, call, "has", |pairs| pairs.iter().any(|pair| pair.0 == name))?))
}

fn length_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Medido: o getter com `this` alheio lança `Type error` simples, sem `code`.
    let count = with_instance::<FormData, _>(call.this_value(), |pairs| pairs.len()).ok_or_else(|| Thrown::type_error("Type error"))?;
    Ok(js_number(count as f64))
}

/// O objeto de `toJSON` de `value` quando é um `FormData`: o que o `console.log` imprime.
pub(crate) fn json_object_of(global_object: &JSGlobalObject, value: JSValue) -> Option<JSValue> {
    crate::runtime::web_iterable::json_object_of::<FormData>(global_object, value)
}

fn to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let pairs = with_entries::<FormData, _>(global_object, call, "toJSON", |pairs| pairs.clone())?;
    Ok(pairs_to_json_object::<FormData>(global_object, pairs, None))
}

/// `FormData.from(input, boundary)` (`FormData__jsFunctionFromMultipartData` do bun): `input` é texto ou binário
/// (`ArrayBuffer`, view, `Blob`); sem `boundary` (ou vazio) o corpo é `urlencoded`, com ele é multipart.
fn from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (input, boundary) = (call.argument(0), call.argument(1));
    if input.is_undefined_or_null() {
        return Err(throw_coded_type_error(global_object, "input must not be empty", "ERR_INVALID_ARG_TYPE"));
    }
    let boundary = if boundary.is_undefined_or_null() {
        None
    } else if let Some(bytes) = input_bytes(boundary) {
        // Medido: `Blob` como boundary não vale (só `ArrayBuffer`/view), diferente do `input`.
        Some(bytes)
    } else if boundary.is_string() {
        Some(string_bytes(global_object, boundary)?)
    } else {
        return Err(throw_coded_type_error(global_object, "boundary must be a string or ArrayBufferView", "ERR_INVALID_ARG_TYPE"));
    };
    let bytes = if let Some(bytes) = binary_bytes(input) {
        bytes
    } else if input.is_string() {
        string_bytes(global_object, input)?
    } else {
        return Err(throw_coded_type_error(global_object, "input must be a string or ArrayBufferView", "ERR_INVALID_ARG_TYPE"));
    };
    match parse_with_boundary(&bytes, boundary.filter(|boundary| !boundary.is_empty())) {
        Ok(entries) => Ok(make_form_data(global_object, entries)),
        Err(message) => Err(throw_error(global_object, &format!("{message} while parsing FormData"))),
    }
}

host_function!(call_fn, call_body);
host_function!(from_fn, from_body);
host_function!(construct_fn, construct_body);
host_function!(append_fn, append_body);
host_function!(set_fn, set_body);
host_function!(delete_fn, delete_body);
host_function!(get_fn, get_body);
host_function!(get_all_fn, get_all_body);
host_function!(has_fn, has_body);
host_function!(length_fn, length_body);
host_function!(to_json_fn, to_json_body);
web_iterable_functions!(FormData);

/// Instala `FormData` no global.
pub fn install_form_data(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let (prototype, constructor) = create_native_class_with_length(global_object, &PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "FormData", 0, call_fn, construct_fn);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    FORM_DATA_PROTOTYPE.with(|slot| *slot.borrow_mut() = Some(prototype.as_value()));
    let methods: [(&str, u32, NativeFunction); 10] = [
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
    ];
    put_enumerable_methods(global_object, &prototype, &methods);
    // `FormData.from` é estática, enumerável, `ReadOnly | DontDelete` (JSDOMFormData.cpp).
    put_direct_native_function_without_transition(
        vm,
        global_object,
        &constructor,
        &Identifier::from_span(vm, b"from"),
        1,
        from_fn,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        READ_ONLY | DONT_DELETE,
    );
    // `toJSON` é a única de dados que não é `writable` nem `configurable` (medido).
    put_direct_native_function_without_transition(
        vm,
        global_object,
        &prototype,
        &Identifier::from_span(vm, b"toJSON"),
        1,
        to_json_fn,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        READ_ONLY | DONT_DELETE,
    );
    put_native_getter(vm, global_object, &prototype, "length", length_fn, Intrinsic::NoIntrinsic, DONT_ENUM | DONT_DELETE);
    put_iterator_alias(global_object, &prototype);
    install_iteration::<FormData>(global_object, &prototype, &ITERATOR_PROTOTYPE_S_INFO, iterator_next_fn);
    // Enumerável, ao contrário de `URLSearchParams` (medido).
    install_global_with_attributes(global_object, "FormData", constructor.as_value(), 0);
}
