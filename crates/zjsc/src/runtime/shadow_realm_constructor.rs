//! Porte de `runtime/ShadowRealmConstructor.{h,cpp}` e `ShadowRealmConstructorInlines.h`: o construtor
//! `ShadowRealm` (um `InternalFunction`, comprimento 0).
//!
//! DIVERGÊNCIA: `constructWithShadowRealmConstructor` usa `globalObject->shadowRealmPrototype()`; o porte não
//! guarda esse protótipo no global, então lê a propriedade `prototype` do próprio construtor, que é
//! `DontDelete|ReadOnly` e vale exatamente o `m_shadowRealmPrototype`. Como no C++, o `newTarget` é ignorado.

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{
    collection_constructor_structure, constructor_cannot_be_called_as_function, create_collection_constructor,
};
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::internal_function::{InternalFunctionRef, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_name::PropertyName;
use crate::runtime::shadow_realm_object::ShadowRealmObject;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `const ClassInfo ShadowRealmConstructor::s_info` (`"Function"`).
pub static SHADOW_REALM_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `callShadowRealm`: `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope, "ShadowRealm")`.
fn call_shadow_realm_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("ShadowRealm")
}

/// `constructWithShadowRealmConstructor`.
fn construct_shadow_realm_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let callee = JSObject::from_cell_id(call.callee()).expect("constructWithShadowRealmConstructor: o callee é o InternalFunction do ShadowRealm (um JSObject)");
    let prototype = callee.get(vm, &PropertyName::from_identifier(&vm.property_names.prototype));
    let structure = ShadowRealmObject::create_structure(vm, Some(global_object), prototype);
    Ok(ShadowRealmObject::create(vm, &structure, global_object).as_value())
}

host_function!(call_shadow_realm, call_shadow_realm_body);
host_function!(construct_shadow_realm, construct_shadow_realm_body);

/// `class ShadowRealmConstructor final : public InternalFunction`: sem campos próprios.
pub struct ShadowRealmConstructor;

impl ShadowRealmConstructor {
    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        collection_constructor_structure(vm, global_object, prototype, &SHADOW_REALM_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, structure, shadowRealmPrototype)`: `finishCreation` com comprimento 0, nome `"ShadowRealm"` e
    /// `prototype` (`DontEnum|DontDelete|ReadOnly`).
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        prototype: &JSObject,
    ) -> InternalFunctionRef {
        create_collection_constructor(
            vm,
            global_object,
            structure,
            prototype,
            "ShadowRealm",
            0,
            call_shadow_realm,
            construct_shadow_realm,
            false,
        )
    }
}
