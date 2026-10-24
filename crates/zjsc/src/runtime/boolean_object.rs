//! Porte de `runtime/BooleanObject.h`, `BooleanObjectInlines.h` e `BooleanObject.cpp`: o objeto
//! `Boolean`, um `JSWrapperObject` com o `JSType` `BooleanObjectType` (a base e o registro estão em
//! `js_wrapper_object.rs`).

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_wrapper_object::{JSWrapperObject, JSWrapperObjectRef};
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `const ClassInfo BooleanObject::s_info`.
pub static BOOLEAN_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "Boolean", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class BooleanObject : public JSWrapperObject`: espaço de nomes de `createStructure`, `create` e do
/// downcast.
pub struct BooleanObject;

impl BooleanObject {
    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        JSWrapperObject::create_structure(vm, global_object, prototype, JSType::BooleanObjectType, &BOOLEAN_OBJECT_S_INFO)
    }

    /// `dynamicDowncast<BooleanObject>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSWrapperObjectRef> {
        JSWrapperObject::from_value_of_type(value, JSType::BooleanObjectType)
    }
}

/// `constructBooleanFromImmediateBoolean(globalObject, immediateBooleanValue)`.
pub fn construct_boolean_from_immediate_boolean(
    vm: &VM,
    global_object: &JSGlobalObject,
    immediate_boolean_value: JSValue,
) -> JSWrapperObjectRef {
    // `BooleanObject::create(vm, structure)` é o `JSWrapperObject::create`.
    let object = JSWrapperObject::create(vm, global_object.boolean_object_structure());
    object.set_internal_value(immediate_boolean_value);
    object
}
