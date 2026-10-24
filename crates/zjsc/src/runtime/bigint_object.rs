//! Porte de `runtime/BigIntObject.h`, `BigIntObjectInlines.h` e `BigIntObject.cpp`: o objeto `BigInt`,
//! um `JSWrapperObject` cuja `Structure` tem o `JSType` `ObjectType` (o `BigIntObject` não tem
//! `JSType` próprio em `JSType.h`) e o `ClassInfo` `"BigInt"`; é o `ClassInfo` que o
//! `dynamicDowncast<BigIntObject>` confere. A base e o registro estão em `js_wrapper_object.rs`.
//!
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_wrapper_object::{JSWrapperObject, JSWrapperObjectRef};
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `const ClassInfo BigIntObject::s_info`.
pub static BIG_INT_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "BigInt", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class BigIntObject final : public JSWrapperObject`: espaço de nomes de `createStructure`, `create` e
/// do downcast.
pub struct BigIntObject;

impl BigIntObject {
    /// `createStructure(vm, globalObject, prototype)` (`BigIntObjectInlines.h`).
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        JSWrapperObject::create_structure(vm, global_object, prototype, JSType::ObjectType, &BIG_INT_OBJECT_S_INFO)
    }

    /// `create(vm, globalObject, bigInt)`: o `BigIntObject(vm, structure)` e o `finishCreation(vm,
    /// bigInt)`, que grava o BigInt como valor interno.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, big_int: JSValue) -> JSWrapperObjectRef {
        debug_assert!(big_int.is_big_int());
        let object = JSWrapperObject::create(vm, global_object.big_int_object_structure());
        object.set_internal_value(big_int);
        object
    }

    /// `dynamicDowncast<BigIntObject>(value)`: o `ClassInfo` da estrutura é o de `BigIntObject`.
    pub fn from_value(value: &JSValue) -> Option<JSWrapperObjectRef> {
        JSWrapperObject::from_value_of_type(value, JSType::ObjectType)
            .filter(|object| std::ptr::eq(object.structure().class_info(), &BIG_INT_OBJECT_S_INFO))
    }
}
