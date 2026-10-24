//! Porte de `runtime/FinalizationRegistryConstructor.{h,cpp}` e `FinalizationRegistryConstructorInlines.h`: o
//! construtor `FinalizationRegistry` (um `InternalFunction`, comprimento 1).
//!
//! A casca e a estrutura base são as de `weak_object_ref_constructor.rs` (ver a DIVERGÊNCIA de
//! `collection_support.rs` sobre `globalObject->finalizationRegistryStructure()`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{
    constructor_cannot_be_called_as_function, create_native_collection_constructor, derived_structure, native_constructor_structure,
};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::js_function::{JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::js_finalization_registry::JSFinalizationRegistry;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `const ClassInfo FinalizationRegistryConstructor::s_info` (`"Function"`).
pub static FINALIZATION_REGISTRY_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `callFinalizationRegistry`: `throwConstructorCannotBeCalledAsFunctionTypeError`.
fn call_finalization_registry_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("FinalizationRegistry")
}

/// `constructFinalizationRegistry`.
fn construct_finalization_registry_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let callback = call.argument(0);
    if !callback.is_callable() {
        return Err(Thrown::type_error("First argument to FinalizationRegistry should be a function"));
    }
    let structure = derived_structure(global_object, call, JSFinalizationRegistry::create_structure)?;
    Ok(JSFinalizationRegistry::create(global_object.vm(), &structure, callback).as_value())
}

host_function!(call_finalization_registry, call_finalization_registry_body);
host_function!(construct_finalization_registry, construct_finalization_registry_body);

/// `class FinalizationRegistryConstructor final : public InternalFunction`: sem campos próprios.
pub struct FinalizationRegistryConstructor;

impl FinalizationRegistryConstructor {
    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        native_constructor_structure(vm, global_object, prototype, &FINALIZATION_REGISTRY_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, structure, prototype)`: `finishCreation` com comprimento 1, nome `"FinalizationRegistry"`.
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        prototype: &JSObject,
    ) -> JSFunctionRef {
        create_native_collection_constructor(
            vm,
            global_object,
            structure,
            prototype,
            "FinalizationRegistry",
            1,
            call_finalization_registry,
            construct_finalization_registry,
            false,
        )
    }
}
