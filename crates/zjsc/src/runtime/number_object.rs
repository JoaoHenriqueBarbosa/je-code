//! Porte de `runtime/NumberObject.h` e `NumberObject.cpp`: o objeto `Number`, um `JSWrapperObject` com o
//! `JSType` `NumberObjectType` (a base e o registro estão em `js_wrapper_object.rs`; o `finishCreation`
//! do C++ só tem asserções).

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_wrapper_object::{JSWrapperObject, JSWrapperObjectRef};
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `const ClassInfo NumberObject::s_info` (a base `JSWrapperObject` não tem `ClassInfo` próprio no C++:
/// `JSInternalFieldObjectImpl` e `JSWrapperObject` herdam o do `JSNonFinalObject`).
pub static NUMBER_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "Number", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class NumberObject : public JSWrapperObject`: espaço de nomes de `createStructure`, `create` e do
/// downcast.
pub struct NumberObject;

impl NumberObject {
    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        JSWrapperObject::create_structure(vm, global_object, prototype, JSType::NumberObjectType, &NUMBER_OBJECT_S_INFO)
    }

    /// `dynamicDowncast<NumberObject>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSWrapperObjectRef> {
        JSWrapperObject::from_value_of_type(value, JSType::NumberObjectType)
    }
}

/// `constructNumber(globalObject, number)`.
pub fn construct_number(vm: &VM, global_object: &JSGlobalObject, number: JSValue) -> JSWrapperObjectRef {
    // `NumberObject::create(vm, structure)` é o `JSWrapperObject::create` (o `finishCreation` do C++ só
    // tem asserções).
    let object = JSWrapperObject::create(vm, global_object.number_object_structure());
    debug_assert!(object.type_() == JSType::NumberObjectType);
    object.set_internal_value(number);
    object
}
