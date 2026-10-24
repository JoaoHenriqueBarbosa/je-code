//! `File` do global. O JavaScriptCore não o define: quem o instala é o bun (WebCore). Medido no bun 1.4.2
//! (`scripts/gen-file-formdata-golden.js`, `tests/golden/file_formdata_bun.tsv`):
//!
//! - propriedade global de dados `writable`, `configurable` e ENUMERÁVEL; construtor nativo `length` 2, `name` "File",
//!   chaves próprias `length`, `name`, `prototype`, protótipo `Function.prototype` (não herda de `Blob`);
//! - `File.prototype` é o MESMO objeto que `Blob.prototype` (então `File.prototype.constructor` é `Blob`); o que separa
//!   os dois é o estado: `instanceof File` só vale para quem foi criado como arquivo (`new File`, o que `slice` de um
//!   arquivo devolve e o que o `FormData` guarda), nunca para um `Blob` comum;
//! - sem `new`: ``Class constructor File cannot be invoked without 'new'``; menos de dois argumentos:
//!   `new File(bits, name) expects at least 2 arguments` (`ERR_INVALID_ARG_TYPE`); as partes seguem as de `Blob`;
//! - `name` passa por `ToString` (sem normalização), `type` como no `Blob`, `lastModified` por `ToNumber` (`NaN` vira 0,
//!   `undefined` é o relógio, `null` é 0), guardado como número de ponto flutuante.
//! O estado das instâncias é o de `Blob` (`blob.rs`): este módulo só constrói e confere.

use std::cell::Cell;

use crate::host_function;
use crate::runtime::blob::{blob_prototype, normalized_type, parts_bytes, register_instance, state_of, BlobState};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{create_collection_constructor, derived_structure, internal_function_structure};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSFinalObject;
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{install_global_with_attributes, instance_structure, throw_coded_type_error};
use crate::runtime::property_name::PropertyName;

static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// As raízes do coletor: o construtor `File`.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.encoded(FILE_CONSTRUCTOR.with(Cell::get));
}

thread_local! {
    /// O construtor `File` do realm, para o `instanceof` conferir o alvo.
    static FILE_CONSTRUCTOR: Cell<EncodedJSValue> = const { Cell::new(0) };
}

fn call_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error("Class constructor File cannot be invoked without 'new'"))
}

fn option_property(global_object: &JSGlobalObject, options: JSValue, name: &str) -> Result<JSValue, Thrown> {
    get_value_property(global_object, options, &PropertyName::from_identifier(&Identifier::from_span(global_object.vm(), name.as_bytes())))
}

fn now_milliseconds() -> f64 {
    (crate::wtf::date_math::current_time_in_nanoseconds() / 1_000_000) as f64
}

/// `lastModified` das opções: ausente é o relógio, `NaN` é 0, o resto o número como veio.
fn last_modified_of(global_object: &JSGlobalObject, options: JSValue) -> Result<f64, Thrown> {
    if !options.is_object() {
        return Ok(now_milliseconds());
    }
    let value = option_property(global_object, options, "lastModified")?;
    if value.is_undefined() {
        return Ok(now_milliseconds());
    }
    let number = pending_or(global_object, value.to_number())?;
    Ok(if number.is_nan() { 0.0 } else { number })
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() < 2 {
        return Err(throw_coded_type_error(global_object, "new File(bits, name) expects at least 2 arguments", "ERR_INVALID_ARG_TYPE"));
    }
    let bytes = parts_bytes(global_object, call.argument(0))?;
    let text = pending_or(global_object, call.argument(1).to_wtf_string())?;
    let name: Vec<u16> = (0..text.length()).map(|index| text.code_unit_at(index)).collect();
    let options = call.argument(2);
    let content_type = if options.is_object() { normalized_type(option_property(global_object, options, "type")?) } else { Vec::new() };
    let last_modified = last_modified_of(global_object, options)?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    register_instance(instance, BlobState { bytes, content_type, name: Some(name), is_file: true, last_modified: Some(last_modified) });
    Ok(instance)
}

host_function!(call_fn, call_body);
host_function!(construct_fn, construct_body);

/// `instanceof File`: só vale para o que foi criado como arquivo; `None` quando o alvo não é o `File` do realm.
pub(crate) fn file_has_instance(target: JSValue, value: JSValue) -> Option<bool> {
    if FILE_CONSTRUCTOR.with(Cell::get) != target.encode() {
        return None;
    }
    Some(value.is_object() && state_of(value).is_some_and(|state| state.is_file))
}

/// Instala `File` no global (depois de `Blob`: o protótipo é o dele).
pub fn install_file(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let prototype = blob_prototype().as_object();
    // Sem `ImplementsDefaultHasInstance`: o `instanceof File` não olha a cadeia (o protótipo é o de `Blob`), e o
    // `JSObject::hasInstance` cai no `customHasInstance` (`file_has_instance`).
    let flags = crate::runtime::internal_function::InternalFunction::STRUCTURE_FLAGS & !crate::runtime::js_type_info::IMPLEMENTS_DEFAULT_HAS_INSTANCE;
    let structure = internal_function_structure(vm, global_object, global_object.function_prototype().as_value(), &CONSTRUCTOR_S_INFO, flags);
    let constructor = create_collection_constructor(vm, global_object, structure, &prototype, "File", 2, call_fn, construct_fn, false);
    FILE_CONSTRUCTOR.with(|slot| slot.set(constructor.as_value().encode()));
    // Enumerável, como `FormData` (medido).
    install_global_with_attributes(global_object, "File", constructor.as_value(), 0);
}
