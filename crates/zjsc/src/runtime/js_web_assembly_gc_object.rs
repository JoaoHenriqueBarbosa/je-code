//! O objeto Wasm GC (struct ou array) quando chega ao JS (`JSWebAssemblyStruct`, `JSWebAssemblyArray`, a base
//! `WebAssemblyGCObjectBase`): célula opaca com protótipo `null`, não extensível e sem propriedades, ligada à
//! referência (índice global no registro compartilhado `GcHeap`, `GC_REF_TAG`).
//!
//! Medido no bun 1.4.2 (`wasm_js_bun.tsv`): `typeof` é `object`, `Object.keys` e `Reflect.ownKeys` vazios,
//! `Object.isFrozen` verdadeiro; escrever (`o.x = 1`, `Reflect.set`, `o[0] = 1`) lança `Cannot set property for
//! WebAssembly GC object`, apagar lança `Cannot delete property ...`, `Object.defineProperty` lança `Cannot define
//! property ...` (`Reflect.defineProperty` dá `false`), `Object.setPrototypeOf` lança `Cannot set prototype of
//! WebAssembly GC object` (`Reflect.setPrototypeOf` dá `false`), `Object.freeze`/`seal`/`preventExtensions` e
//! `Reflect.preventExtensions` lançam `Cannot run preventExtensions operation on WebAssembly GC object`. O mesmo
//! objeto GC volta como o mesmo objeto JS (`===`), e passá-lo de volta ao wasm devolve a mesma referência.
//!
//! Como as demais classes do `WebAssembly` da ponte, a célula é um `IntlInstance` (com `ClassInfo` própria, que é
//! como os ganchos de `JSObject` a reconhecem).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::Thrown;
use crate::runtime::intl_support::IntlInstance;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, PutError, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_null, JSValue};
use crate::runtime::structure::{Structure, StructureRef};
use crate::wasm::wasm_instance::{gc_ref, gc_ref_index};

/// `const ClassInfo` do objeto GC (`"Object"`).
pub static WEB_ASSEMBLY_GC_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

const SET_MESSAGE: &str = "Cannot set property for WebAssembly GC object";
const DELETE_MESSAGE: &str = "Cannot delete property for WebAssembly GC object";
const DEFINE_MESSAGE: &str = "Cannot define property for WebAssembly GC object";
const SET_PROTOTYPE_MESSAGE: &str = "Cannot set prototype of WebAssembly GC object";
const PREVENT_EXTENSIONS_MESSAGE: &str = "Cannot run preventExtensions operation on WebAssembly GC object";

/// A referência que o objeto JS embrulha: o índice no registro de células GC compartilhado do VM.
struct GcObjectState {
    index: usize,
}

thread_local! {
    /// A estrutura compartilhada: protótipo `null`, não extensível.
    static STRUCTURE: RefCell<Option<StructureRef>> = const { RefCell::new(None) };
    /// Índice global da célula -> o objeto JS, para `mk() === h(mk())` e o mesmo objeto de volta, mesmo entre
    /// instâncias. As entradas nunca saem (sem GC).
    static OBJECTS: RefCell<HashMap<usize, JSValue>> = RefCell::new(HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): a estrutura e os objetos são células do programa.
/// As raízes do coletor: os objetos GC do wasm expostos ao JS.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    OBJECTS.with(|objects| objects.borrow().values().for_each(|value| marker.value(*value)));
}

pub(crate) fn reset_for_program() {
    let _ = STRUCTURE.try_with(|structure| *structure.borrow_mut() = None);
    let _ = OBJECTS.try_with(|objects| objects.borrow_mut().clear());
}

fn structure(global_object: &JSGlobalObject) -> StructureRef {
    STRUCTURE.with(|slot| {
        slot.borrow_mut()
            .get_or_insert_with(|| {
                let vm = global_object.vm();
                let base = Structure::create(
                    vm,
                    Some(global_object),
                    js_null(),
                    TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
                    &WEB_ASSEMBLY_GC_OBJECT_S_INFO,
                );
                Structure::prevent_extensions_transition(vm, &base)
            })
            .clone()
    })
}

/// O objeto JS do objeto GC `reference` da instância (o que já existe, ou um novo); `None` se não é uma
/// referência a objeto GC.
pub(crate) fn gc_object_for(global_object: &JSGlobalObject, reference: u64) -> Option<JSValue> {
    let index = gc_ref_index(reference)?;
    if let Some(value) = OBJECTS.with(|objects| objects.borrow().get(&index).copied()) {
        return Some(value);
    }
    let value = IntlInstance::create(global_object.vm(), &structure(global_object), Box::new(GcObjectState { index })).as_value();
    OBJECTS.with(|objects| objects.borrow_mut().insert(index, value));
    Some(value)
}

/// A referência de um objeto JS que embrulha um objeto GC (índice global no registro compartilhado); `None` para
/// qualquer outro valor.
pub(crate) fn gc_object_of(value: JSValue) -> Option<u64> {
    let cell = IntlInstance::from_value(&value)?;
    let state = cell.state::<GcObjectState>()?;
    Some(gc_ref(state.index))
}

/// O objeto é um objeto GC embrulhado?
fn is_gc_object(object: &JSObject) -> bool {
    std::ptr::eq(object.class_info(), &WEB_ASSEMBLY_GC_OBJECT_S_INFO)
}

/// `[[Set]]` (`put`, `putByIndex`): sempre lança, mesmo em modo sloppy e via `Reflect.set`.
pub(crate) fn put(object: &JSObject) -> Option<Result<bool, PutError>> {
    is_gc_object(object).then_some(Err(PutError::TypeError(SET_MESSAGE)))
}

/// `[[Delete]]`: sempre lança.
pub(crate) fn delete_property(object: &JSObject) -> Option<Result<bool, PutError>> {
    is_gc_object(object).then_some(Err(PutError::TypeError(DELETE_MESSAGE)))
}

/// `[[DefineOwnProperty]]`: `false`, ou `TypeError` quando o chamador pediu que lançasse.
pub(crate) fn define_own_property(object: &JSObject, throw_exception: bool) -> Option<Result<bool, PutError>> {
    is_gc_object(object).then_some(if throw_exception { Err(PutError::TypeError(DEFINE_MESSAGE)) } else { Ok(false) })
}

/// `[[PreventExtensions]]`: sempre lança.
pub(crate) fn prevent_extensions(object: &JSObject) -> Option<Thrown> {
    is_gc_object(object).then(|| Thrown::type_error(PREVENT_EXTENSIONS_MESSAGE))
}

/// `[[SetPrototypeOf]]`: sempre lança (o protótipo é `null` e fixo).
pub(crate) fn set_prototype(object: &JSObject) -> Option<Thrown> {
    is_gc_object(object).then(|| Thrown::type_error(SET_PROTOTYPE_MESSAGE))
}
