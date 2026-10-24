//! Porte de `runtime/WeakObjectRefConstructor.{h,cpp}` e `WeakObjectRefConstructorInlines.h`: o construtor
//! `WeakRef` (um `InternalFunction`, comprimento 1).
//!
//! A casca (`ClassInfo`, `createStructure`, `finishCreation`) é a de `collection_support.rs`, a mesma dos
//! construtores de coleção; a estrutura base vem do `prototype` do próprio construtor (ver a DIVERGÊNCIA de
//! `collection_support.rs` sobre `globalObject->weakObjectRefStructure()`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{
    constructor_cannot_be_called_as_function, create_native_collection_constructor, derived_structure, native_constructor_structure,
};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::js_function::{JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_weak_map::can_be_held_weakly;
use crate::runtime::js_weak_object_ref::JSWeakObjectRef;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `const ClassInfo WeakObjectRefConstructor::s_info` (`"Function"`).
pub static WEAK_OBJECT_REF_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `callWeakRef`: `throwConstructorCannotBeCalledAsFunctionTypeError`.
fn call_weak_ref_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("WeakRef")
}

/// `constructWeakRef`.
fn construct_weak_ref_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target = call.argument(0);
    if !can_be_held_weakly(target) {
        return Err(Thrown::type_error("First argument to WeakRef should be an object or a non-registered symbol"));
    }
    let structure = derived_structure(global_object, call, JSWeakObjectRef::create_structure)?;
    Ok(JSWeakObjectRef::create(global_object.vm(), &structure, target).as_value())
}

host_function!(call_weak_ref, call_weak_ref_body);
host_function!(construct_weak_ref, construct_weak_ref_body);

/// `class WeakObjectRefConstructor final : public InternalFunction`: sem campos próprios.
pub struct WeakObjectRefConstructor;

impl WeakObjectRefConstructor {
    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        native_constructor_structure(vm, global_object, prototype, &WEAK_OBJECT_REF_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, structure, prototype)`: `finishCreation` com comprimento 1, nome `"WeakRef"` e `prototype`.
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        prototype: &JSObject,
    ) -> JSFunctionRef {
        create_native_collection_constructor(vm, global_object, structure, prototype, "WeakRef", 1, call_weak_ref, construct_weak_ref, false)
    }
}
